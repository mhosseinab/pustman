use std::sync::Arc;

use rustls::crypto::CryptoProvider;
use tokio::net::TcpListener;
use tonic::service::Routes;
use tonic::transport::server::TcpIncoming;
use tonic::transport::{Certificate, ClientTlsConfig, Endpoint, Identity, Server, ServerTlsConfig};

use super::ensure_crypto_provider;

/// Key-exchange groups tell the providers apart where cipher suites do not: ring and
/// aws-lc-rs share every suite, but only aws-lc-rs offers X25519MLKEM768.
fn kx_group_names(provider: &CryptoProvider) -> Vec<rustls::NamedGroup> {
    provider
        .kx_groups
        .iter()
        .map(|group| group.name())
        .collect()
}

#[tokio::test]
async fn tonic_channel_and_reqwest_share_one_crypto_provider() {
    ensure_crypto_provider();
    let installed = CryptoProvider::get_default().expect("a process-default provider");
    ensure_crypto_provider();
    let after_second_call = CryptoProvider::get_default().expect("a process-default provider");
    assert!(
        Arc::ptr_eq(installed, after_second_call),
        "a second call must not replace the provider"
    );
    // reqwest's `rustls` feature is aws-lc-rs: the default must be that provider.
    let installed_groups = kx_group_names(installed);
    assert!(
        installed_groups.contains(&rustls::NamedGroup::X25519MLKEM768),
        "the default provider is aws-lc-rs, not ring: {installed_groups:?}"
    );
    assert_eq!(
        installed_groups,
        kx_group_names(&rustls::crypto::aws_lc_rs::default_provider())
    );

    // An in-process tonic server with an rcgen certificate for `127.0.0.1` and no
    // services: every call is answered, which is all a handshake needs.
    let certified = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()])
        .expect("a self-signed certificate");
    let certificate_pem = certified.cert.pem();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a listener");
    let port = listener.local_addr().expect("an address").port();
    let server = tokio::spawn(
        Server::builder()
            .tls_config(ServerTlsConfig::new().identity(Identity::from_pem(
                &certificate_pem,
                certified.signing_key.serialize_pem(),
            )))
            .expect("a TLS server config")
            .add_routes(Routes::default())
            .serve_with_incoming(TcpIncoming::from(listener)),
    );

    // tonic: an eager connect is TCP, TLS and the HTTP/2 preface.
    let _channel = Endpoint::from_shared(format!("https://127.0.0.1:{port}"))
        .expect("an endpoint")
        .tls_config(ClientTlsConfig::new().ca_certificate(Certificate::from_pem(&certificate_pem)))
        .expect("a TLS endpoint")
        .connect()
        .await
        .expect("the tonic channel connects over TLS");

    // reqwest: the same server answers over HTTP/2.
    let client = reqwest::Client::builder()
        .no_proxy()
        .add_root_certificate(
            reqwest::Certificate::from_pem(certificate_pem.as_bytes())
                .expect("a certificate for reqwest"),
        )
        .build()
        .expect("a reqwest client");
    let response = client
        .post(format!("https://127.0.0.1:{port}/pustman.Missing/Call"))
        .header("content-type", "application/grpc")
        .send()
        .await
        .expect("an HTTPS response");
    assert_eq!(response.version(), reqwest::Version::HTTP_2);

    server.abort();
}
