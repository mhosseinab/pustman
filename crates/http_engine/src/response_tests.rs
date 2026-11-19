//! A [`Response`] carries response headers and body bytes, so its `Debug` output keeps
//! them out of logs (AGENTS.md › Security).

use crate::{Header, Response, ResponseBody, ResponseVersion, Timings};

#[test]
fn debug_output_hides_body_and_cookie_values() {
    let response = Response {
        status: 200,
        reason: "OK".to_string(),
        http_version: ResponseVersion::Http11,
        headers: vec![Header::new("Set-Cookie", "session=secret-cookie")],
        body: ResponseBody::Bytes(b"\"token\":\"secret-body\"".to_vec()),
        timings: Timings::default(),
        network: None,
        redirects: Vec::new(),
    };

    let debug = format!("{response:?}");

    assert!(!debug.contains("secret-cookie"), "cookie leaked: {debug}");
    assert!(!debug.contains("secret-body"), "body leaked: {debug}");
    assert!(debug.contains("Set-Cookie"), "header names stay visible");
    assert!(debug.contains("200"), "status stays visible");
}
