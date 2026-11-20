//! The transport's cookie jar, end to end (REQ-HTTP-08, REQ-NET-04): a response's
//! `Set-Cookie` is captured against the URL that answered, and the `Cookie` header the
//! jar applied is part of the resolved request as sent — on the next send and on every
//! redirect hop. Both tests run against loopback servers and assert on the requests
//! those servers received: what arrived is the resolved request as it went on the wire.
//! Cookie values are secrets (PRD §7), so a failing assertion prints redacted headers.

use std::sync::{Arc, Mutex};

use url::Url;

use crate::cookie_test_support::{ScriptedReply, ScriptedServer, get_request};
use crate::cookies::CookieJar;
use crate::request::Header;
use crate::test_server::TestServer;
use crate::{ReqwestTransport, Transport};

/// A `Set-Cookie` is captured against the URL that answered it, and the next request to
/// that URL goes out with the cookie — applied into the resolved request's own header
/// list, so the `Cookie` header actually sent is recorded there (REQ-NET-04).
#[tokio::test]
async fn set_cookie_captured_and_sent_on_next_request() -> Result<(), Box<dyn std::error::Error>> {
    let server = ScriptedServer::start(vec![
        ScriptedReply {
            path: "/".to_string(),
            status: 200,
            headers: vec![Header::new(
                "set-cookie",
                "sid=sid-value-REDACTED-example; Path=/",
            )],
            body: Vec::new(),
        },
        ScriptedReply {
            path: "/next".to_string(),
            status: 200,
            headers: Vec::new(),
            body: Vec::new(),
        },
    ])
    .await?;
    let jar = Arc::new(Mutex::new(CookieJar::new()));
    let transport =
        ReqwestTransport::new(tokio::runtime::Handle::current()).with_cookie_jar(Arc::clone(&jar));

    let response = transport.send(get_request(server.url())?).await?;
    assert_eq!(response.status, 200);
    let response = transport
        .send(get_request(&server.url().join("/next")?)?)
        .await?;
    assert_eq!(response.status, 200);

    let received = server.requests();
    assert_eq!(received.len(), 2);
    assert!(
        !received[0]
            .headers
            .iter()
            .any(|header| header.name.eq_ignore_ascii_case("cookie")),
        "the first request carried a Cookie header before anything was captured: {:?}",
        received[0].headers
    );
    assert!(
        received[1].headers.iter().any(|header| {
            header.name.eq_ignore_ascii_case("cookie")
                && header.value == "sid=sid-value-REDACTED-example"
        }),
        "the applied Cookie header is missing from the resolved request as sent: {:?}",
        received[1].headers
    );
    Ok(())
}

/// The `Set-Cookie` of a redirect response is applied on the next hop, and a hop that
/// crosses hosts carries only cookies matching the new host — the jar's RFC 6265 domain
/// matching (`cookies.rs`), pinned end to end here.
#[tokio::test]
async fn redirect_hop_cookie_applied() -> Result<(), Box<dyn std::error::Error>> {
    // The chain ends on a second server reached under a different host name (`localhost`
    // where the first hop is `127.0.0.1`), so the last hop is a cross-host hop.
    let final_server = TestServer::start().await?;
    let final_port = final_server
        .url()
        .port()
        .ok_or("the loopback server's URL carries no port")?;
    let cross_host = Url::parse(&format!("http://localhost:{final_port}/target"))?;
    let hop_server = ScriptedServer::start(vec![
        ScriptedReply {
            path: "/".to_string(),
            status: 302,
            headers: vec![
                Header::new("set-cookie", "sid=sid-value-REDACTED-example; Path=/"),
                Header::new("location", "/hop/1"),
            ],
            body: Vec::new(),
        },
        ScriptedReply {
            path: "/hop/1".to_string(),
            status: 302,
            headers: vec![Header::new("location", cross_host.as_str())],
            body: Vec::new(),
        },
    ])
    .await?;

    let jar = Arc::new(Mutex::new(CookieJar::new()));
    // A cookie already in the workspace jar, scoped to the host the chain ends on.
    jar.lock()
        .expect("the jar is not poisoned")
        .store_response_cookies(
            &Url::parse("http://localhost/")?,
            &["local=local-value-REDACTED-example; Path=/".to_string()],
        );
    let transport =
        ReqwestTransport::new(tokio::runtime::Handle::current()).with_cookie_jar(Arc::clone(&jar));

    let response = transport.send(get_request(hop_server.url())?).await?;
    assert_eq!(response.status, 200);

    let hop_requests = hop_server.requests();
    assert_eq!(hop_requests.len(), 2);
    assert!(
        hop_requests[1].headers.iter().any(|header| {
            header.name.eq_ignore_ascii_case("cookie")
                && header.value == "sid=sid-value-REDACTED-example"
        }),
        "the redirect hop sent no Cookie header from the jar: {:?}",
        hop_requests[1].headers
    );

    let final_requests = final_server.requests();
    assert_eq!(final_requests.len(), 1);
    let cookie_headers: Vec<&Header> = final_requests[0]
        .headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("cookie"))
        .collect();
    assert_eq!(
        cookie_headers.len(),
        1,
        "the cross-host hop sent no single Cookie header: {:?}",
        final_requests[0].headers
    );
    assert_eq!(
        cookie_headers[0].value,
        "local=local-value-REDACTED-example"
    );
    assert!(
        !cookie_headers[0].value.contains("sid="),
        "a cookie scoped to 127.0.0.1 crossed to localhost: {:?}",
        cookie_headers[0]
    );
    Ok(())
}
