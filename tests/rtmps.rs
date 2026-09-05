//! RTMPS integration test.
//!
//! The server in this crate speaks plain TCP only, so TLS is terminated by an
//! in-process proxy (the role stunnel or nginx would play in a deployment):
//!
//! ```text
//! client --TLS--> acceptor --TCP--> RtmpServer
//! ```
//!
//! The certificate is self-signed for `localhost`, so the client must be given
//! it as an extra root via `ClientConfig::tls_root_cert`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use rtmp_rs::server::handler::LoggingHandler;
use rtmp_rs::{ClientConfig, RtmpConnector, RtmpServer, ServerConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

/// Reserve an ephemeral port by binding and dropping a listener.
///
/// Racy in principle, but the port is re-bound immediately by `RtmpServer`.
async fn free_port() -> SocketAddr {
    TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
}

async fn wait_for_listener(addr: SocketAddr) {
    for _ in 0..50 {
        if TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("server never started listening on {addr}");
}

struct Harness {
    /// TLS endpoint the client connects to
    tls_addr: SocketAddr,
    /// Self-signed cert the acceptor presents
    cert: CertificateDer<'static>,
}

async fn start() -> Harness {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert = certified.cert.der().clone();
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        certified.signing_key.serialize_der(),
    ));

    // Plain RTMP server
    let rtmp_addr = free_port().await;
    let server = RtmpServer::new(ServerConfig::with_addr(rtmp_addr), LoggingHandler);
    tokio::spawn(async move {
        let _ = server.run().await;
    });
    wait_for_listener(rtmp_addr).await;

    // TLS terminator in front of it
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], key)
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(server_config));
    let tls_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tls_addr = tls_listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = tls_listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(socket).await else {
                    return;
                };
                let Ok(mut upstream) = TcpStream::connect(rtmp_addr).await else {
                    return;
                };
                let _ = tokio::io::copy_bidirectional(&mut tls, &mut upstream).await;
            });
        }
    });

    Harness { tls_addr, cert }
}

#[tokio::test]
async fn connects_over_rtmps_with_extra_root() {
    let h = start().await;

    let url = format!("rtmps://localhost:{}/live/test", h.tls_addr.port());
    let config = ClientConfig::new(&url).tls_root_cert(h.cert.clone());

    let connector = RtmpConnector::connect(config).await.unwrap();
    assert_eq!(connector.stream_id(), 0);
}

#[tokio::test]
async fn rejects_untrusted_certificate() {
    let h = start().await;

    let url = format!("rtmps://localhost:{}/live/test", h.tls_addr.port());
    let config = ClientConfig::new(&url);

    match RtmpConnector::connect(config).await {
        Ok(_) => panic!("connection succeeded without trusting the self-signed certificate"),
        Err(rtmp_rs::Error::Io(_)) => {}
        Err(other) => panic!("expected TLS verification failure, got {other:?}"),
    }
}
