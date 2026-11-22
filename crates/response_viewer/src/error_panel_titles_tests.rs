//! The callout names a failure the way a person would (PRD §8.6, #229): "Invalid URL",
//! "TLS error" — never the `TransportError` variant name the code happens to use. The
//! title debug key derives from the rendered title text, so what the test finds on
//! screen is exactly what the user reads.

use std::time::Duration;

use gpui::{TestAppContext, VisualTestContext};
use http_engine::TransportError;
use ui::test_support::{draw_window, init_test};

use super::{error_kind, error_title_selector};
use crate::ResponseView;

#[gpui::test]
fn error_callout_titles_name_the_failure_for_humans(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| ResponseView::new(cx));

    for (error, title) in failures() {
        let raw_kind = error_kind(&error);
        view.update(cx, |view, cx| view.set_error(error.clone(), cx));
        draw_window(cx);

        assert!(
            shown(cx, error_title_selector(title)),
            "the callout titles the failure \"{title}\" ({raw_kind})"
        );
        if raw_kind != title {
            assert!(
                !shown(cx, error_title_selector(raw_kind)),
                "the callout never shows the raw enum name \"{raw_kind}\""
            );
        }
    }
}

/// One failure per kind, with the title the callout must show for it.
fn failures() -> Vec<(TransportError, &'static str)> {
    vec![
        (
            TransportError::InvalidUrl {
                url: String::from("https://example.test"),
            },
            "Invalid URL",
        ),
        (
            TransportError::Dns {
                host: String::from("api.example.test"),
            },
            "DNS error",
        ),
        (
            TransportError::Connect {
                address: String::from("api.example.test:443"),
            },
            "Connection failed",
        ),
        (
            TransportError::Tls {
                reason: String::from("certificate expired"),
            },
            "TLS error",
        ),
        (
            TransportError::Timeout {
                after: Duration::from_secs(30),
            },
            "Timeout",
        ),
        (
            TransportError::TooManyRedirects { chain: Vec::new() },
            "Too many redirects",
        ),
        (
            TransportError::Proxy {
                reason: String::from("the proxy refused the request"),
            },
            "Proxy error",
        ),
        (
            TransportError::Protocol {
                detail: String::from("unexpected frame"),
            },
            "Protocol error",
        ),
        (
            TransportError::Io {
                detail: String::from("connection reset"),
            },
            "I/O error",
        ),
        (
            TransportError::FileNotFound {
                path: String::from("body.json"),
            },
            "File not found",
        ),
        (
            TransportError::UnresolvedSecret {
                names: vec![String::from("token")],
            },
            "Unresolved secret",
        ),
        (TransportError::Cancelled, "Cancelled"),
    ]
}

/// Whether anything is on screen under this debug key (the `error_panel_tests.rs`
/// pattern: `VisualTestContext::debug_bounds` takes a `&'static str`, so the selector is
/// leaked — fine for a test's short life).
fn shown(cx: &mut VisualTestContext, selector: String) -> bool {
    cx.debug_bounds(selector.leak()).is_some()
}
