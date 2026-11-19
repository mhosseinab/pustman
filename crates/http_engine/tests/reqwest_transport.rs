//! Integration tests for [`ReqwestTransport`] against the loopback [`TestServer`].
//!
//! Wrapped in `mod reqwest_transport` so the test filter `reqwest_transport` matches
//! exactly this module's tests.

use std::time::Duration;

use http_engine::{
    Header, RequestBody, ReqwestTransport, ResolvedRequest, SendSettings, TestServer, Transport,
    TransportError,
};

#[cfg(test)]
mod reqwest_transport {
    use super::*;

    /// A POST with a JSON body reaches the loopback server, and its answer comes back.
    #[tokio::test]
    async fn sends_post_with_json_body_to_loopback() -> Result<(), Box<dyn std::error::Error>> {
        let server = TestServer::start().await?;
        let transport = ReqwestTransport::new(tokio::runtime::Handle::current());

        let body = br#"{"name":"pustman"}"#.to_vec();
        let request = ResolvedRequest::new(
            api_model::Method::Post,
            server.url().as_str(),
            vec![
                Header::new("content-type", "application/json"),
                Header::new("accept", "application/json"),
            ],
            RequestBody::Bytes(body.clone()),
            SendSettings::default(),
        )?;

        let response = transport.send(request).await?;

        assert_eq!(response.status, 200);
        assert_eq!(response.body.as_bytes(), body.as_slice());

        let received = server.requests();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].method, "POST");
        assert_eq!(received[0].path_and_query, "/");
        assert_eq!(received[0].body, body);
        assert!(
            received[0]
                .headers
                .iter()
                .any(|header| header.name == "content-type" && header.value == "application/json"),
            "the JSON content type went missing: {:?}",
            received[0].headers
        );
        Ok(())
    }

    /// Nothing listening on the address maps to `Connect`, naming that address (§8.6).
    #[tokio::test]
    async fn connect_refused_maps_to_connect_error() -> Result<(), Box<dyn std::error::Error>> {
        let port = port_with_no_listener();
        let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
        let request = ResolvedRequest::new(
            api_model::Method::Get,
            &format!("http://127.0.0.1:{port}/users"),
            Vec::new(),
            RequestBody::None,
            SendSettings::default(),
        )?;

        let error = transport.send(request).await.unwrap_err();

        match error {
            TransportError::Connect { address } => {
                assert_eq!(address, format!("127.0.0.1:{port}"));
            }
            other => panic!("expected a Connect error, got {other:?}"),
        }
        Ok(())
    }

    /// Six redirects with `max_redirects` of five fail with `TooManyRedirects`, and the
    /// chain names all six hops — the sixth is the one that was not followed.
    #[tokio::test]
    async fn six_redirects_with_max_five_fail_with_chain_of_six()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = TestServer::start_redirects(&[302, 302, 302, 302, 302, 302]).await?;
        let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
        let request = ResolvedRequest::new(
            api_model::Method::Get,
            server.url().as_str(),
            Vec::new(),
            RequestBody::None,
            SendSettings {
                max_redirects: 5,
                ..SendSettings::default()
            },
        )?;

        let error = transport.send(request).await.unwrap_err();

        match error {
            TransportError::TooManyRedirects { chain } => {
                assert_eq!(chain.len(), 6);
                for (index, hop) in chain.iter().enumerate() {
                    assert_eq!(hop.status, 302, "hop {index} did not see the redirect");
                    let expected_url = if index == 0 {
                        server.url().clone()
                    } else {
                        server.url().join(&format!("hop/{index}"))?
                    };
                    assert_eq!(hop.url, expected_url, "hop {index} was sent elsewhere");
                    assert!(hop.location.is_some(), "hop {index} has no location");
                    assert!(hop.timing.is_some(), "hop {index} has no timing");
                }
            }
            other => panic!("expected a TooManyRedirects error, got {other:?}"),
        }
        // The five hops the limit allows were sent, and the sixth answer ended the send.
        assert_eq!(server.requests().len(), 6);
        Ok(())
    }

    /// Every hop of a chain is recorded — method, URL, status, headers, the `Location`
    /// followed and the timing — and the method rules are the REQ-HTTP-09 ones: 307
    /// keeps the method and the body, 302 turns a POST into a GET unless
    /// `follow_original_method` asks for the original.
    #[tokio::test]
    async fn redirect_chain_records_each_hop() -> Result<(), Box<dyn std::error::Error>> {
        let server = TestServer::start_redirects(&[307, 302]).await?;
        let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
        let body = br#"{"name":"pustman"}"#.to_vec();

        let request = ResolvedRequest::new(
            api_model::Method::Post,
            server.url().as_str(),
            Vec::new(),
            RequestBody::Bytes(body.clone()),
            SendSettings::default(),
        )?;
        let response = transport.send(request).await?;

        assert_eq!(response.status, 200);
        assert_eq!(response.redirects.len(), 2);

        let first = &response.redirects[0];
        assert_eq!(first.method, api_model::Method::Post);
        assert_eq!(first.url, *server.url());
        assert_eq!(first.status, 307);
        assert_eq!(first.location, Some(server.url().join("hop/1")?));
        assert!(
            first
                .headers
                .iter()
                .any(|header| header.name == "location" && header.value == "/hop/1"),
            "the hop headers lost the Location: {:?}",
            first.headers
        );
        assert!(first.timing.is_some(), "the first hop has no timing");

        let second = &response.redirects[1];
        assert_eq!(second.method, api_model::Method::Post);
        assert_eq!(second.url, server.url().join("hop/1")?);
        assert_eq!(second.status, 302);
        assert_eq!(second.location, Some(server.url().join("hop/2")?));
        assert!(second.timing.is_some(), "the second hop has no timing");

        let received = server.requests();
        assert_eq!(received.len(), 3);
        assert_eq!(received[0].method, "POST");
        assert_eq!(received[0].path_and_query, "/");
        assert_eq!(received[0].body, body);
        // The 307 kept the method and the body on the second request.
        assert_eq!(received[1].method, "POST");
        assert_eq!(received[1].path_and_query, "/hop/1");
        assert_eq!(received[1].body, body);
        // The 302 turned the POST into a GET and dropped the body.
        assert_eq!(received[2].method, "GET");
        assert_eq!(received[2].path_and_query, "/hop/2");
        assert!(
            received[2].body.is_empty(),
            "the GET kept the POST body: {:?}",
            received[2].body
        );

        // `follow_original_method` keeps the method even across a 302.
        let request = ResolvedRequest::new(
            api_model::Method::Post,
            server.url().as_str(),
            Vec::new(),
            RequestBody::Bytes(body.clone()),
            SendSettings {
                follow_original_method: true,
                ..SendSettings::default()
            },
        )?;
        let response = transport.send(request).await?;

        assert_eq!(response.status, 200);
        assert_eq!(response.redirects.len(), 2);
        assert_eq!(response.redirects[1].method, api_model::Method::Post);
        let received = server.requests();
        assert_eq!(received.len(), 6);
        assert_eq!(received[5].method, "POST");
        assert_eq!(received[5].path_and_query, "/hop/2");
        assert_eq!(received[5].body, body);
        Ok(())
    }

    /// A server that answers slower than the request timeout maps to `Timeout`, naming
    /// the timeout it gave up after (§8.6).
    #[tokio::test]
    async fn timeout_maps_to_timeout_error() -> Result<(), Box<dyn std::error::Error>> {
        let server = TestServer::start_delayed(Duration::from_secs(5)).await?;
        let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
        let request = ResolvedRequest::new(
            api_model::Method::Get,
            server.url().as_str(),
            Vec::new(),
            RequestBody::None,
            SendSettings {
                timeout: Duration::from_millis(100),
                ..SendSettings::default()
            },
        )?;

        let error = transport.send(request).await.unwrap_err();

        match error {
            TransportError::Timeout { after } => {
                assert_eq!(after, Duration::from_millis(100));
            }
            other => panic!("expected a Timeout error, got {other:?}"),
        }
        Ok(())
    }

    /// A self-signed certificate fails the handshake and lands in `Tls` (§8.6): by
    /// default certificates are verified and nothing trusts this one (REQ-HTTP-10).
    #[tokio::test]
    async fn self_signed_fails_with_tls_error() -> Result<(), Box<dyn std::error::Error>> {
        let server = TestServer::start_tls().await?;
        let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
        let request = ResolvedRequest::new(
            api_model::Method::Get,
            server.url().as_str(),
            Vec::new(),
            RequestBody::None,
            SendSettings::default(),
        )?;

        let error = transport.send(request).await.unwrap_err();

        match error {
            TransportError::Tls { reason } => {
                assert!(!reason.is_empty(), "a TLS failure names its reason");
            }
            other => panic!("expected a Tls error, got {other:?}"),
        }
        Ok(())
    }

    /// Turning verification off for one request (REQ-HTTP-10) lets the same self-signed
    /// server through — and it is per request, never global: the same transport still
    /// fails a request that kept the default. The response also reports what the
    /// connection could report (REQ-NET-02) and nothing more (ADR-0003).
    #[tokio::test]
    async fn self_signed_succeeds_when_verify_disabled_for_request()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = TestServer::start_tls().await?;
        let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
        let body = br#"{"name":"pustman"}"#.to_vec();
        let request = ResolvedRequest::new(
            api_model::Method::Post,
            server.url().as_str(),
            Vec::new(),
            RequestBody::Bytes(body.clone()),
            SendSettings {
                verify_tls: false,
                ..SendSettings::default()
            },
        )?;

        let response = transport.send(request).await?;

        assert_eq!(response.status, 200);
        assert_eq!(response.body.as_bytes(), body.as_slice());
        assert_eq!(server.requests().len(), 1);

        let network = response
            .network
            .expect("the connection reports network info");
        assert_eq!(network.remote_ip.as_deref(), Some("127.0.0.1"));
        assert_eq!(network.remote_port, server.url().port_or_known_default());
        assert!(
            network.local_address.is_some(),
            "the local socket address is reported: {network:?}"
        );
        assert!(
            network.alpn.is_none(),
            "reqwest reports no ALPN, so it stays None (ADR-0003): {network:?}"
        );
        let tls = network.tls.expect("an https send reports its TLS facts");
        assert!(
            !tls.verification_enabled,
            "verification was off for this request"
        );
        assert!(
            matches!(tls.protocol.as_deref(), Some("TLS 1.2" | "TLS 1.3")),
            "the negotiated version is reported, got {:?}",
            tls.protocol
        );
        assert!(
            tls.cipher.is_none(),
            "reqwest reports no cipher suite, so it stays None (ADR-0003): {tls:?}"
        );
        assert!(
            tls.certificates.is_none(),
            "reqwest reports no parseable chain, so it stays None (ADR-0003): {tls:?}"
        );

        // The toggle is per request: a request that kept the default still verifies.
        let verified = ResolvedRequest::new(
            api_model::Method::Get,
            server.url().as_str(),
            Vec::new(),
            RequestBody::None,
            SendSettings::default(),
        )?;
        let error = transport.send(verified).await.unwrap_err();
        assert!(
            matches!(error, TransportError::Tls { .. }),
            "the default request still verifies, got {error:?}"
        );
        Ok(())
    }

    /// REQ-NET-01: a phase the transport cannot measure is `None`, never estimated. This
    /// send really did a DNS lookup, a TCP connect and a TLS handshake — and `reqwest`
    /// reports none of those phases (ADR-0003), so all of them stay `None`. Only the
    /// phases the transport measures around `reqwest`'s own futures are filled, and they
    /// add up.
    #[tokio::test]
    async fn timings_are_none_or_measured_never_estimated() -> Result<(), Box<dyn std::error::Error>>
    {
        let server = TestServer::start_tls().await?;
        let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
        let request = ResolvedRequest::new(
            api_model::Method::Get,
            server.url().as_str(),
            Vec::new(),
            RequestBody::None,
            SendSettings {
                verify_tls: false,
                ..SendSettings::default()
            },
        )?;

        let wall_started = std::time::Instant::now();
        let response = transport.send(request).await?;
        let wall = wall_started.elapsed();

        assert_eq!(response.status, 200);
        let timings = response.timings;

        // Not observable through reqwest (ADR-0003): a duration here would be an
        // estimate of a phase nobody measured.
        assert!(
            timings.prepare.is_none(),
            "prepare is unmeasured: {timings:?}"
        );
        assert!(timings.dns.is_none(), "DNS is unmeasured: {timings:?}");
        assert!(
            timings.connect.is_none(),
            "connect is unmeasured: {timings:?}"
        );
        assert!(timings.tls.is_none(), "TLS is unmeasured: {timings:?}");

        // Measured phases are present and consistent with each other and with the wall
        // clock of the send.
        let total = timings.total.expect("total is measured");
        let first_byte = timings.first_byte.expect("first byte is measured");
        let download = timings.download.expect("download is measured");
        assert!(
            first_byte <= total,
            "first byte is inside the exchange: {timings:?}"
        );
        assert!(
            download <= total,
            "download is inside the exchange: {timings:?}"
        );
        assert!(
            first_byte + download <= total,
            "the parts fit inside the whole: {timings:?}"
        );
        assert!(
            total <= wall,
            "the exchange happened inside the send: {timings:?}"
        );
        Ok(())
    }

    /// A redirect to another origin — a different port is a different origin — drops
    /// the credential headers before the next hop goes out, the guarantee reqwest's own
    /// redirect handling makes (`remove_sensitive_headers`): `Cookie`, `Cookie2`,
    /// `Proxy-Authorization` and `Authorization` never reach the other server, while an
    /// ordinary header still does.
    #[tokio::test]
    async fn a_cross_origin_redirect_drops_credentials_and_cookies()
    -> Result<(), Box<dyn std::error::Error>> {
        let other = TestServer::start().await?;
        let first = TestServer::start_redirect_to(other.url()).await?;
        let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
        let request = ResolvedRequest::new(
            api_model::Method::Get,
            first.url().as_str(),
            vec![
                Header::new("Cookie", "session=cookie-secret"),
                Header::new("Cookie2", "session2=cookie2-secret"),
                Header::new("Proxy-Authorization", "Basic proxy-secret"),
                Header::new("Authorization", "Bearer authorization-secret"),
                Header::new("X-Trace", "trace-kept"),
            ],
            RequestBody::None,
            SendSettings::default(),
        )?;

        let response = transport.send(request).await?;

        assert_eq!(response.status, 200);
        assert_eq!(response.redirects.len(), 1);
        // The premise: the first hop really carried the credentials.
        let sent = first.requests();
        assert_eq!(sent.len(), 1);
        assert!(
            sent[0]
                .headers
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case("cookie")),
            "the first hop carried no cookie: {:?}",
            sent[0].headers
        );
        // And none of them crossed to the other origin.
        let forwarded = other.requests();
        assert_eq!(
            forwarded.len(),
            1,
            "the redirected hop reached the other server"
        );
        let headers = &forwarded[0].headers;
        for secret in [
            "cookie-secret",
            "cookie2-secret",
            "proxy-secret",
            "authorization-secret",
        ] {
            assert!(
                !headers.iter().any(|header| header.value.contains(secret)),
                "`{secret}` reached the other origin: {headers:?}"
            );
        }
        assert!(
            headers
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case("x-trace")
                    && header.value == "trace-kept"),
            "an ordinary header stopped crossing: {headers:?}"
        );
        Ok(())
    }

    /// The other half of the rule: a same-origin hop keeps today's behavior — the
    /// cookies and the proxy credentials ride along, and only `Authorization` needs its
    /// opt-in (REQ-HTTP-09).
    #[tokio::test]
    async fn a_same_origin_redirect_keeps_cookies_and_proxy_credentials()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = TestServer::start_redirects(&[302]).await?;
        let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
        let request = ResolvedRequest::new(
            api_model::Method::Get,
            server.url().as_str(),
            vec![
                Header::new("Cookie", "session=cookie-secret"),
                Header::new("Proxy-Authorization", "Basic proxy-secret"),
                Header::new("Authorization", "Bearer authorization-secret"),
            ],
            RequestBody::None,
            SendSettings::default(),
        )?;

        let response = transport.send(request).await?;

        assert_eq!(response.status, 200);
        let received = server.requests();
        assert_eq!(received.len(), 2);
        let second = &received[1].headers;
        assert!(
            second
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case("cookie")
                    && header.value == "session=cookie-secret"),
            "a same-origin hop lost its cookie: {second:?}"
        );
        assert!(
            second
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case("proxy-authorization")),
            "a same-origin hop lost its proxy credentials: {second:?}"
        );
        assert!(
            !second
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case("authorization")),
            "authorization crossed a hop without its opt-in: {second:?}"
        );
        Ok(())
    }

    /// `keep_authorization` is the one override and only for `Authorization`
    /// (REQ-HTTP-09): across a cross-origin hop the authorization still crosses when
    /// the request asked for it — the cookies and the proxy credentials never do.
    #[tokio::test]
    async fn keep_authorization_crosses_a_cross_origin_hop_but_cookies_never_do()
    -> Result<(), Box<dyn std::error::Error>> {
        let other = TestServer::start().await?;
        let first = TestServer::start_redirect_to(other.url()).await?;
        let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
        let request = ResolvedRequest::new(
            api_model::Method::Get,
            first.url().as_str(),
            vec![
                Header::new("Cookie", "session=cookie-secret"),
                Header::new("Proxy-Authorization", "Basic proxy-secret"),
                Header::new("Authorization", "Bearer authorization-secret"),
            ],
            RequestBody::None,
            SendSettings {
                keep_authorization: true,
                ..SendSettings::default()
            },
        )?;

        let response = transport.send(request).await?;

        assert_eq!(response.status, 200);
        let forwarded = other.requests();
        assert_eq!(forwarded.len(), 1);
        let headers = &forwarded[0].headers;
        assert!(
            headers
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case("authorization")
                    && header.value == "Bearer authorization-secret"),
            "keep_authorization lost its authorization: {headers:?}"
        );
        assert!(
            !headers
                .iter()
                .any(|header| header.value.contains("cookie-secret")
                    || header.value.contains("proxy-secret")),
            "cookies or proxy credentials crossed anyway: {headers:?}"
        );
        Ok(())
    }

    /// A 302 turns the POST into a body-less GET, and the body's own headers go with it
    /// — `Content-Length` and `Content-Type`, whatever their case. A rewritten GET that
    /// still announces bytes it never sends is what hangs servers mid-redirect.
    #[tokio::test]
    async fn a_method_rewrite_drops_content_headers_whatever_their_case()
    -> Result<(), Box<dyn std::error::Error>> {
        let server = TestServer::start_redirects(&[302]).await?;
        let transport = ReqwestTransport::new(tokio::runtime::Handle::current());
        let body = br#"{"name":"pustman"}"#.to_vec();
        let request = ResolvedRequest::new(
            api_model::Method::Post,
            server.url().as_str(),
            vec![
                Header::new("Content-Length", "18"),
                Header::new("Content-Type", "application/json"),
            ],
            RequestBody::Bytes(body),
            SendSettings::default(),
        )?;

        let response = transport.send(request).await?;

        assert_eq!(response.status, 200);
        let received = server.requests();
        assert_eq!(received.len(), 2);
        // The premise: the capitalized names went out on the POST...
        assert!(
            received[0]
                .headers
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case("content-length")),
            "the POST carried no content length: {:?}",
            received[0].headers
        );
        assert!(
            received[0]
                .headers
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case("content-type")),
            "the POST carried no content type: {:?}",
            received[0].headers
        );
        // ... and the rewritten GET carries neither.
        assert_eq!(received[1].method, "GET");
        assert!(
            !received[1]
                .headers
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case("content-length")
                    || header.name.eq_ignore_ascii_case("content-type")),
            "the body-less GET kept the body's headers: {:?}",
            received[1].headers
        );
        Ok(())
    }

    /// A port nothing listens on: bind one to learn its number, then let it go again.
    fn port_with_no_listener() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        listener.local_addr().expect("the bound address").port()
    }
}
