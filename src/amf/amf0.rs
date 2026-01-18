//! AMF0 encoder and decoder
//!
//! AMF0 is the original Action Message Format used in Flash/RTMP.
//! Reference: AMF0 File Format Specification (amf0-file-format-specification.pdf)
//!
//! Type Markers:
//! ```text
//! 0x00 - Number (IEEE 754 double)
//! 0x01 - Boolean
//! 0x02 - String (UTF-8, 16-bit length prefix)
//! 0x03 - Object (key-value pairs until 0x000009)
//! 0x04 - MovieClip (reserved, not supported)
//! 0x05 - Null
//! 0x06 - Undefined
//! 0x07 - Reference (16-bit index)
//! 0x08 - ECMA Array (associative array)
//! 0x09 - Object End (0x000009 sequence)
//! 0x0A - Strict Array (dense array)
//! 0x0B - Date (double + timezone)
//! 0x0C - Long String (UTF-8, 32-bit length prefix)
//! 0x0D - Unsupported
//! 0x0E - RecordSet (reserved, not supported)
//! 0x0F - XML Document
//! 0x10 - Typed Object (class name + properties)
//! 0x11 - AVM+ (switch to AMF3)
//! ```

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::collections::HashMap;

use crate::error::AmfError;
use super::value::AmfValue;

// AMF0 type markers
const MARKER_NUMBER: u8 = 0x00;
const MARKER_BOOLEAN: u8 = 0x01;
const MARKER_STRING: u8 = 0x02;
const MARKER_OBJECT: u8 = 0x03;
const MARKER_NULL: u8 = 0x05;
const MARKER_UNDEFINED: u8 = 0x06;
const MARKER_REFERENCE: u8 = 0x07;
const MARKER_ECMA_ARRAY: u8 = 0x08;
const MARKER_OBJECT_END: u8 = 0x09;
const MARKER_STRICT_ARRAY: u8 = 0x0A;
const MARKER_DATE: u8 = 0x0B;
const MARKER_LONG_STRING: u8 = 0x0C;
const MARKER_UNSUPPORTED: u8 = 0x0D;
const MARKER_XML_DOCUMENT: u8 = 0x0F;
const MARKER_TYPED_OBJECT: u8 = 0x10;
const MARKER_AVMPLUS: u8 = 0x11;

/// Maximum nesting depth for objects/arrays (prevent stack overflow)
const MAX_NESTING_DEPTH: usize = 64;

/// AMF0 decoder with lenient parsing mode
pub struct Amf0Decoder {
    /// Reference table for object references
    references: Vec<AmfValue>,
    /// Enable lenient parsing for encoder quirks
    lenient: bool,
    /// Current nesting depth
    depth: usize,
}

impl Amf0Decoder {
    /// Create a new decoder with default settings
    pub fn new() -> Self {
        Self {
            references: Vec::new(),
            lenient: true, // Default to lenient for OBS/encoder compatibility
            depth: 0,
        }
    }

    /// Create decoder with explicit lenient mode setting
    pub fn with_lenient(lenient: bool) -> Self {
        Self {
            references: Vec::new(),
            lenient,
            depth: 0,
        }
    }

    /// Reset decoder state (call between messages)
    pub fn reset(&mut self) {
        self.references.clear();
        self.depth = 0;
    }

    /// Decode a single AMF0 value from the buffer
    pub fn decode(&mut self, buf: &mut Bytes) -> Result<AmfValue, AmfError> {
        if buf.is_empty() {
            return Err(AmfError::UnexpectedEof);
        }

        self.depth += 1;
        if self.depth > MAX_NESTING_DEPTH {
            return Err(AmfError::NestingTooDeep);
        }

        let marker = buf.get_u8();
        let result = self.decode_value(marker, buf);
        self.depth -= 1;
        result
    }

    /// Decode all values from buffer until exhausted
    pub fn decode_all(&mut self, buf: &mut Bytes) -> Result<Vec<AmfValue>, AmfError> {
        let mut values = Vec::new();
        while buf.has_remaining() {
            values.push(self.decode(buf)?);
        }
        Ok(values)
    }

    fn decode_value(&mut self, marker: u8, buf: &mut Bytes) -> Result<AmfValue, AmfError> {
        match marker {
            MARKER_NUMBER => self.decode_number(buf),
            MARKER_BOOLEAN => self.decode_boolean(buf),
            MARKER_STRING => self.decode_string(buf),
            MARKER_OBJECT => self.decode_object(buf),
            MARKER_NULL => Ok(AmfValue::Null),
            MARKER_UNDEFINED => Ok(AmfValue::Undefined),
            MARKER_REFERENCE => self.decode_reference(buf),
            MARKER_ECMA_ARRAY => self.decode_ecma_array(buf),
            MARKER_STRICT_ARRAY => self.decode_strict_array(buf),
            MARKER_DATE => self.decode_date(buf),
            MARKER_LONG_STRING => self.decode_long_string(buf),
            MARKER_UNSUPPORTED => Ok(AmfValue::Undefined),
            MARKER_XML_DOCUMENT => self.decode_xml(buf),
            MARKER_TYPED_OBJECT => self.decode_typed_object(buf),
            MARKER_AVMPLUS => {
                // AMF3 value embedded in AMF0 stream
                // For now, skip and return null (full AMF3 support in amf3.rs)
                Ok(AmfValue::Null)
            }
            _ => {
                if self.lenient {
                    // Skip unknown marker in lenient mode
                    Ok(AmfValue::Undefined)
                } else {
                    Err(AmfError::UnknownMarker(marker))
                }
            }
        }
    }

    fn decode_number(&mut self, buf: &mut Bytes) -> Result<AmfValue, AmfError> {
        if buf.remaining() < 8 {
            return Err(AmfError::UnexpectedEof);
        }
        Ok(AmfValue::Number(buf.get_f64()))
    }

    fn decode_boolean(&mut self, buf: &mut Bytes) -> Result<AmfValue, AmfError> {
        if buf.is_empty() {
            return Err(AmfError::UnexpectedEof);
        }
        Ok(AmfValue::Boolean(buf.get_u8() != 0))
    }

    fn decode_string(&mut self, buf: &mut Bytes) -> Result<AmfValue, AmfError> {
        let s = self.read_utf8(buf)?;
        Ok(AmfValue::String(s))
    }

    fn decode_long_string(&mut self, buf: &mut Bytes) -> Result<AmfValue, AmfError> {
        let s = self.read_utf8_long(buf)?;
        Ok(AmfValue::String(s))
    }

    fn decode_object(&mut self, buf: &mut Bytes) -> Result<AmfValue, AmfError> {
        let mut properties = HashMap::new();

        // Track this object for potential references
        let obj_index = self.references.len();
        self.references.push(AmfValue::Null); // Placeholder

        loop {
            let key = self.read_utf8(buf)?;

            // Check for object end marker
            if key.is_empty() {
                if buf.is_empty() {
                    if self.lenient {
                        // OBS sometimes omits the object end marker
                        break;
                    }
                    return Err(AmfError::UnexpectedEof);
                }
                let end_marker = buf.get_u8();
                if end_marker == MARKER_OBJECT_END {
                    break;
                } else if self.lenient {
                    // Some encoders omit the end marker, treat as end
                    // Put the byte back conceptually by continuing
                    break;
                } else {
                    return Err(AmfError::InvalidObjectEnd);
                }
            }

            let value = self.decode(buf)?;
            properties.insert(key, value);
        }

        let obj = AmfValue::Object(properties);
        self.references[obj_index] = obj.clone();
        Ok(obj)
    }

    fn decode_ecma_array(&mut self, buf: &mut Bytes) -> Result<AmfValue, AmfError> {
        if buf.remaining() < 4 {
            return Err(AmfError::UnexpectedEof);
        }

        // ECMA array count hint (not always accurate)
        let _count = buf.get_u32();

        // Track for references
        let arr_index = self.references.len();
        self.references.push(AmfValue::Null);

        let mut properties = HashMap::new();

        loop {
            let key = self.read_utf8(buf)?;

            if key.is_empty() {
                if buf.is_empty() {
                    if self.lenient {
                        break;
                    }
                    return Err(AmfError::UnexpectedEof);
                }
                let end_marker = buf.get_u8();
                if end_marker == MARKER_OBJECT_END {
                    break;
                } else if self.lenient {
                    break;
                } else {
                    return Err(AmfError::InvalidObjectEnd);
                }
            }

            let value = self.decode(buf)?;
            properties.insert(key, value);
        }

        let arr = AmfValue::EcmaArray(properties);
        self.references[arr_index] = arr.clone();
        Ok(arr)
    }

    fn decode_strict_array(&mut self, buf: &mut Bytes) -> Result<AmfValue, AmfError> {
        if buf.remaining() < 4 {
            return Err(AmfError::UnexpectedEof);
        }

        let count = buf.get_u32() as usize;

        // Track for references
        let arr_index = self.references.len();
        self.references.push(AmfValue::Null);

        let mut elements = Vec::with_capacity(count.min(1024)); // Cap initial allocation
        for _ in 0..count {
            elements.push(self.decode(buf)?);
        }

        let arr = AmfValue::Array(elements);
        self.references[arr_index] = arr.clone();
        Ok(arr)
    }

    fn decode_date(&mut self, buf: &mut Bytes) -> Result<AmfValue, AmfError> {
        if buf.remaining() < 10 {
            return Err(AmfError::UnexpectedEof);
        }

        let timestamp = buf.get_f64();
        let _timezone = buf.get_i16(); // Timezone offset (deprecated, usually 0)

        Ok(AmfValue::Date(timestamp))
    }

    fn decode_reference(&mut self, buf: &mut Bytes) -> Result<AmfValue, AmfError> {
        if buf.remaining() < 2 {
            return Err(AmfError::UnexpectedEof);
        }

        let index = buf.get_u16() as usize;
        if index >= self.references.len() {
            return Err(AmfError::InvalidReference(index as u16));
        }

        Ok(self.references[index].clone())
    }

    fn decode_xml(&mut self, buf: &mut Bytes) -> Result<AmfValue, AmfError> {
        let s = self.read_utf8_long(buf)?;
        Ok(AmfValue::Xml(s))
    }

    fn decode_typed_object(&mut self, buf: &mut Bytes) -> Result<AmfValue, AmfError> {
        let class_name = self.read_utf8(buf)?;

        // Track for references
        let obj_index = self.references.len();
        self.references.push(AmfValue::Null);

        let mut properties = HashMap::new();

        loop {
            let key = self.read_utf8(buf)?;

            if key.is_empty() {
                if buf.is_empty() {
                    if self.lenient {
                        break;
                    }
                    return Err(AmfError::UnexpectedEof);
                }
                let end_marker = buf.get_u8();
                if end_marker == MARKER_OBJECT_END {
                    break;
                } else if self.lenient {
                    break;
                } else {
                    return Err(AmfError::InvalidObjectEnd);
                }
            }

            let value = self.decode(buf)?;
            properties.insert(key, value);
        }

        let obj = AmfValue::TypedObject { class_name, properties };
        self.references[obj_index] = obj.clone();
        Ok(obj)
    }

    /// Read UTF-8 string with 16-bit length prefix
    fn read_utf8(&mut self, buf: &mut Bytes) -> Result<String, AmfError> {
        if buf.remaining() < 2 {
            return Err(AmfError::UnexpectedEof);
        }

        let len = buf.get_u16() as usize;
        if buf.remaining() < len {
            return Err(AmfError::UnexpectedEof);
        }

        let bytes = buf.copy_to_bytes(len);
        String::from_utf8(bytes.to_vec()).map_err(|_| AmfError::InvalidUtf8)
    }

    /// Read UTF-8 string with 32-bit length prefix
    fn read_utf8_long(&mut self, buf: &mut Bytes) -> Result<String, AmfError> {
        if buf.remaining() < 4 {
            return Err(AmfError::UnexpectedEof);
        }

        let len = buf.get_u32() as usize;
        if buf.remaining() < len {
            return Err(AmfError::UnexpectedEof);
        }

        let bytes = buf.copy_to_bytes(len);
        String::from_utf8(bytes.to_vec()).map_err(|_| AmfError::InvalidUtf8)
    }
}

impl Default for Amf0Decoder {
    fn default() -> Self {
        Self::new()
    }
}

/// AMF0 encoder
pub struct Amf0Encoder {
    buf: BytesMut,
}

impl Amf0Encoder {
    /// Create a new encoder
    pub fn new() -> Self {
        Self {
            buf: BytesMut::with_capacity(256),
        }
    }

    /// Create encoder with specific capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: BytesMut::with_capacity(capacity),
        }
    }

    /// Get the encoded bytes and reset encoder
    pub fn finish(&mut self) -> Bytes {
        self.buf.split().freeze()
    }

    /// Get current encoded length
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Check if encoder is empty
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Encode a single AMF0 value
    pub fn encode(&mut self, value: &AmfValue) {
        match value {
            AmfValue::Null => {
                self.buf.put_u8(MARKER_NULL);
            }
            AmfValue::Undefined => {
                self.buf.put_u8(MARKER_UNDEFINED);
            }
            AmfValue::Boolean(b) => {
                self.buf.put_u8(MARKER_BOOLEAN);
                self.buf.put_u8(if *b { 1 } else { 0 });
            }
            AmfValue::Number(n) => {
                self.buf.put_u8(MARKER_NUMBER);
                self.buf.put_f64(*n);
            }
            AmfValue::Integer(i) => {
                // AMF0 doesn't have integer type, encode as number
                self.buf.put_u8(MARKER_NUMBER);
                self.buf.put_f64(*i as f64);
            }
            AmfValue::String(s) => {
                if s.len() > 0xFFFF {
                    // Long string
                    self.buf.put_u8(MARKER_LONG_STRING);
                    self.buf.put_u32(s.len() as u32);
                } else {
                    self.buf.put_u8(MARKER_STRING);
                    self.buf.put_u16(s.len() as u16);
                }
                self.buf.put_slice(s.as_bytes());
            }
            AmfValue::Object(props) => {
                self.buf.put_u8(MARKER_OBJECT);
                for (key, val) in props {
                    self.write_utf8(key);
                    self.encode(val);
                }
                // Object end marker
                self.buf.put_u16(0); // Empty key
                self.buf.put_u8(MARKER_OBJECT_END);
            }
            AmfValue::EcmaArray(props) => {
                self.buf.put_u8(MARKER_ECMA_ARRAY);
                self.buf.put_u32(props.len() as u32);
                for (key, val) in props {
                    self.write_utf8(key);
                    self.encode(val);
                }
                self.buf.put_u16(0);
                self.buf.put_u8(MARKER_OBJECT_END);
            }
            AmfValue::Array(elements) => {
                self.buf.put_u8(MARKER_STRICT_ARRAY);
                self.buf.put_u32(elements.len() as u32);
                for elem in elements {
                    self.encode(elem);
                }
            }
            AmfValue::Date(timestamp) => {
                self.buf.put_u8(MARKER_DATE);
                self.buf.put_f64(*timestamp);
                self.buf.put_i16(0); // Timezone (deprecated)
            }
            AmfValue::Xml(s) => {
                self.buf.put_u8(MARKER_XML_DOCUMENT);
                self.buf.put_u32(s.len() as u32);
                self.buf.put_slice(s.as_bytes());
            }
            AmfValue::TypedObject { class_name, properties } => {
                self.buf.put_u8(MARKER_TYPED_OBJECT);
                self.write_utf8(class_name);
                for (key, val) in properties {
                    self.write_utf8(key);
                    self.encode(val);
                }
                self.buf.put_u16(0);
                self.buf.put_u8(MARKER_OBJECT_END);
            }
            AmfValue::ByteArray(_) => {
                // ByteArray is AMF3-only, encode as null in AMF0
                self.buf.put_u8(MARKER_NULL);
            }
        }
    }

    /// Encode multiple values
    pub fn encode_all(&mut self, values: &[AmfValue]) {
        for value in values {
            self.encode(value);
        }
    }

    /// Write UTF-8 string with 16-bit length prefix (no type marker)
    fn write_utf8(&mut self, s: &str) {
        let len = s.len().min(0xFFFF);
        self.buf.put_u16(len as u16);
        self.buf.put_slice(&s.as_bytes()[..len]);
    }
}

impl Default for Amf0Encoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to encode a single value
pub fn encode(value: &AmfValue) -> Bytes {
    let mut encoder = Amf0Encoder::new();
    encoder.encode(value);
    encoder.finish()
}

/// Convenience function to encode multiple values
pub fn encode_all(values: &[AmfValue]) -> Bytes {
    let mut encoder = Amf0Encoder::new();
    encoder.encode_all(values);
    encoder.finish()
}

/// Convenience function to decode a single value
pub fn decode(data: &[u8]) -> Result<AmfValue, AmfError> {
    let mut decoder = Amf0Decoder::new();
    let mut buf = Bytes::copy_from_slice(data);
    decoder.decode(&mut buf)
}

/// Convenience function to decode all values
pub fn decode_all(data: &[u8]) -> Result<Vec<AmfValue>, AmfError> {
    let mut decoder = Amf0Decoder::new();
    let mut buf = Bytes::copy_from_slice(data);
    decoder.decode_all(&mut buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_number_roundtrip() {
        let value = AmfValue::Number(42.5);
        let encoded = encode(&value);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_string_roundtrip() {
        let value = AmfValue::String("hello world".into());
        let encoded = encode(&value);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_boolean_roundtrip() {
        let value = AmfValue::Boolean(true);
        let encoded = encode(&value);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_null_roundtrip() {
        let value = AmfValue::Null;
        let encoded = encode(&value);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_object_roundtrip() {
        let mut props = HashMap::new();
        props.insert("name".to_string(), AmfValue::String("test".into()));
        props.insert("value".to_string(), AmfValue::Number(123.0));
        let value = AmfValue::Object(props);

        let encoded = encode(&value);
        let decoded = decode(&encoded).unwrap();

        // Compare as objects (order may differ)
        if let (AmfValue::Object(orig), AmfValue::Object(dec)) = (&value, &decoded) {
            assert_eq!(orig.len(), dec.len());
            for (k, v) in orig {
                assert_eq!(dec.get(k), Some(v));
            }
        } else {
            panic!("Expected objects");
        }
    }

    #[test]
    fn test_array_roundtrip() {
        let value = AmfValue::Array(vec![
            AmfValue::Number(1.0),
            AmfValue::String("two".into()),
            AmfValue::Boolean(true),
        ]);
        let encoded = encode(&value);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn test_multiple_values() {
        let values = vec![
            AmfValue::String("connect".into()),
            AmfValue::Number(1.0),
            AmfValue::Null,
        ];

        let encoded = encode_all(&values);
        let decoded = decode_all(&encoded).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn test_long_string() {
        let long_str = "x".repeat(70000);
        let value = AmfValue::String(long_str.clone());
        let encoded = encode(&value);
        let decoded = decode(&encoded).unwrap();
        assert_eq!(decoded, AmfValue::String(long_str));
    }
}
