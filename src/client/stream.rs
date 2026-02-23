//! Stream abstraction for plain TCP and TLS connections.

use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

#[cfg(feature = "tls")]
use tokio_rustls::client::TlsStream;

/// A stream that is either a plain TCP connection or a TLS-wrapped connection.
pub enum RtmpStream {
    /// Plain TCP connection (rtmp://)
    Tcp(TcpStream),
    /// TLS-wrapped connection (rtmps://)
    #[cfg(feature = "tls")]
    Tls(TlsStream<TcpStream>),
}

impl AsyncRead for RtmpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            RtmpStream::Tcp(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "tls")]
            RtmpStream::Tls(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for RtmpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            RtmpStream::Tcp(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(feature = "tls")]
            RtmpStream::Tls(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            RtmpStream::Tcp(s) => Pin::new(s).poll_flush(cx),
            #[cfg(feature = "tls")]
            RtmpStream::Tls(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            RtmpStream::Tcp(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(feature = "tls")]
            RtmpStream::Tls(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}
