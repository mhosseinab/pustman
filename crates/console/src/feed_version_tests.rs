//! The version the raw request view draws (#114): the request line names the version the
//! exchange negotiated — the version the send actually used — instead of the hardcoded
//! HTTP/1.1 every send used to show, whatever it ran on.
//!
//! `Decision:` the version comes from the exchange's response (`Response::http_version`,
//! the version the transport read off the wire — `reqwest_transport.rs`'s `version_of`),
//! not from the request's `HttpVersion` setting: that setting is what the send *allowed*
//! and `Auto` leaves the choice to the negotiation, so it cannot say what ran (S04/R04).
//! `ResolvedRequest` carries no negotiated version and the response is where the send
//! already reports one, so nothing in `http_engine` has to change.

use api_model::SecretValues;
use gpui::TestAppContext;
use http_engine::{
    AppliedAuth, RequestBody, ResolvedRequest, Response, ResponseBody, ResponseVersion,
    SendOutcome, SendSettings,
};

use crate::{ConsoleStore, record_outcome};

/// The `200 OK` of one `GET /users` exchange that negotiated `version`, with no headers
/// and no body — everything the raw view's request line needs and nothing more.
fn outcome(version: ResponseVersion) -> SendOutcome {
    SendOutcome {
        request: ResolvedRequest::new(
            api_model::Method::Get,
            "https://example.test/users",
            Vec::new(),
            RequestBody::None,
            SendSettings::default(),
        )
        .expect("the fixture URL parses"),
        response: Response {
            http_version: version,
            ..Response::new(200, "OK", ResponseBody::Bytes(b"ok".to_vec()))
        },
        warnings: Vec::new(),
        auth: AppliedAuth::None,
        secrets_used: SecretValues::new(),
    }
}

/// Record `outcome` the way a finished send is recorded, and report the first line of
/// the raw request the entry carries.
fn recorded_request_line(cx: &mut TestAppContext, outcome: &SendOutcome) -> String {
    cx.update(|cx| {
        let init = settings::SettingsStore::init(cx, None);
        assert_eq!(init, Ok(()), "the store starts from the bundled defaults");
        crate::init(cx);
        record_outcome(outcome, cx);
        cx.global::<ConsoleStore>()
            .entries()
            .front()
            .expect("the send records one entry")
            .raw_request
            .lines()
            .next()
            .unwrap_or_default()
            .to_string()
    })
}

/// The raw request names the version the exchange negotiated: HTTP/1.0 and HTTP/2 sends
/// no longer show HTTP/1.1 request text (REQ-CON-03).
#[gpui::test]
fn raw_request_names_the_version_the_send_negotiated(cx: &mut TestAppContext) {
    for (version, request_line) in [
        (ResponseVersion::Http10, "GET /users HTTP/1.0"),
        (ResponseVersion::Http11, "GET /users HTTP/1.1"),
        (ResponseVersion::Http2, "GET /users HTTP/2"),
    ] {
        let outcome = outcome(version);
        assert_eq!(
            recorded_request_line(cx, &outcome),
            request_line,
            "an exchange on {version:?} shows its own version on the request line"
        );
    }
}
