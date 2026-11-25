use std::sync::Arc;

use futures::{SinkExt as _, StreamExt as _};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio_tungstenite::Connector;
use tokio_tungstenite::tungstenite::Message;

use super::ensure_crypto_provider;

/// A server certificate for `127.0.0.1` that only these tests trust.
fn self_signed() -> (tokio_rustls::TlsAcceptor, CertificateDer<'static>) {
    let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()])
        .expect("a self-signed certificate");
    let certificate = certified.cert.der().clone();
    let key = PrivateKeyDer::Pkcs8(certified.signing_key.serialize_der().into());
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![certificate.clone()], key)
        .expect("a TLS server config");
    (
        tokio_rustls::TlsAcceptor::from(Arc::new(config)),
        certificate,
    )
}

fn suite_names(provider: &CryptoProvider) -> Vec<rustls::CipherSuite> {
    provider
        .cipher_suites
        .iter()
        .map(|suite| suite.suite())
        .collect()
}

#[tokio::test]
async fn websocket_and_reqwest_share_one_crypto_provider() {
    ensure_crypto_provider();
    let installed = CryptoProvider::get_default().expect("a process-default provider");
    ensure_crypto_provider();
    let after_second_call = CryptoProvider::get_default().expect("a process-default provider");
    assert!(
        Arc::ptr_eq(installed, after_second_call),
        "a second call must not replace the provider"
    );
    // reqwest's `rustls` feature is aws-lc-rs: the default must be that provider.
    assert_eq!(
        suite_names(installed),
        suite_names(&rustls::crypto::aws_lc_rs::default_provider())
    );

    let (acceptor, certificate) = self_signed();

    // reqwest: one HTTPS exchange.
    let https_listener = TcpListener::bind("127.0.0.1:0").await.expect("a listener");
    let https_port = https_listener.local_addr().expect("an address").port();
    let https_server = tokio::spawn({
        let acceptor = acceptor.clone();
        async move {
            let (tcp, _) = https_listener.accept().await.expect("a connection");
            let mut tls = acceptor.accept(tcp).await.expect("a TLS handshake");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = tls.read(&mut buffer).await.expect("the request");
                assert!(read > 0, "the client hung up mid-request");
                request.extend_from_slice(buffer.get(..read).expect("in bounds"));
            }
            tls.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                .await
                .expect("the response");
            tls.shutdown().await.expect("a TLS close");
        }
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .add_root_certificate(
            reqwest::Certificate::from_der(&certificate).expect("a certificate for reqwest"),
        )
        .build()
        .expect("a reqwest client");
    let body = client
        .get(format!("https://127.0.0.1:{https_port}/"))
        .send()
        .await
        .expect("an HTTPS response")
        .text()
        .await
        .expect("a body");
    assert_eq!(body, "ok");
    https_server.await.expect("the HTTPS server task");

    // tokio-tungstenite: one WSS message.
    let wss_listener = TcpListener::bind("127.0.0.1:0").await.expect("a listener");
    let wss_port = wss_listener.local_addr().expect("an address").port();
    let wss_server = tokio::spawn(async move {
        let (tcp, _) = wss_listener.accept().await.expect("a connection");
        let tls = acceptor.accept(tcp).await.expect("a TLS handshake");
        let mut socket = tokio_tungstenite::accept_async(tls)
            .await
            .expect("a WebSocket upgrade");
        socket
            .send(Message::text("hello"))
            .await
            .expect("a message");
        socket.close(None).await.expect("a close");
    });
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).expect("the test root");
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let (mut socket, _) = tokio_tungstenite::connect_async_tls_with_config(
        format!("wss://127.0.0.1:{wss_port}/"),
        None,
        false,
        Some(Connector::Rustls(Arc::new(config))),
    )
    .await
    .expect("a WSS connection");
    let message = socket
        .next()
        .await
        .expect("a message")
        .expect("a readable message");
    assert_eq!(message.to_text().expect("a text message"), "hello");
    wss_server.await.expect("the WSS server task");
}
