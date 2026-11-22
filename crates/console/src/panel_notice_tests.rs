//! The Console details pane's notice section (#115): a feed notice — a warning, an error
//! message or "Cancelled" — is a message, not wire text, so expanding one shows it under
//! its own `Notice` section and never under a `Response` section the wire never sent. A
//! wire entry keeps its request and response sections, so the two shapes cannot collapse
//! into one.
//!
//! `Decision:` the selector names below are spelled out as literals (they are
//! `debug_selector` values in `panel.rs`) so this test compiles — and fails — against the
//! details pane as it stands (the `panel_toolbar_tests.rs` shape).

use std::time::Duration;

use gpui::{Entity, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use super::ConsolePanel;
use crate::{ConsoleEntry, ConsoleLevel, ConsoleStore};

/// The details sections' debug selectors, as `panel.rs` draws them.
const NOTICE_SECTION: &str = "console-details-notice";
const REQUEST_SECTION: &str = "console-details-request";
const RESPONSE_SECTION: &str = "console-details-response";

/// A notice entry: a message of `level` with no request of its own — the shape the feed's
/// warnings, error records and cancellation records push.
fn notice(level: ConsoleLevel, message: &str) -> ConsoleEntry {
    ConsoleEntry {
        level,
        protocol: "http".to_string(),
        method: None,
        url: "https://api.test/users".to_string(),
        status: None,
        time: None,
        size: None,
        raw_request: String::new(),
        raw_response: message.to_string(),
    }
}

/// A wire entry: a request and a response, each as it went on the wire.
fn wire_entry() -> ConsoleEntry {
    ConsoleEntry {
        level: ConsoleLevel::Info,
        protocol: "http".to_string(),
        method: Some(api_model::Method::Get),
        url: "https://api.test/users".to_string(),
        status: Some(200),
        time: Some(Duration::from_millis(12)),
        size: Some(34),
        raw_request: "GET /users HTTP/1.1\r\nHost: api.test\r\n\r\n".to_string(),
        raw_response: "HTTP/1.1 200 OK\r\n\r\n[]".to_string(),
    }
}

/// A console panel over the real settings, theme and store, with `entry` pushed and
/// expanded — what a user gets after clicking the row.
fn expanded_details(
    entry: ConsoleEntry,
    cx: &mut TestAppContext,
) -> (Entity<ConsolePanel>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        text_input::init(cx);
        crate::init(cx);
    });
    let (panel, cx) = cx.add_window_view(ConsolePanel::new);
    cx.update(|_, cx| ConsoleStore::push(cx, entry));
    panel.update(cx, |panel, cx| panel.toggle_expanded(0, cx));
    draw_window(cx);
    (panel, cx)
}

#[gpui::test]
async fn notice_expands_under_its_own_section_not_a_response_section(cx: &mut TestAppContext) {
    let (_panel, cx) = expanded_details(notice(ConsoleLevel::Warning, "Unresolved variables"), cx);

    assert!(
        cx.debug_bounds(NOTICE_SECTION).is_some(),
        "a notice is drawn under its own Notice section"
    );
    assert!(
        cx.debug_bounds(RESPONSE_SECTION).is_none(),
        "a notice is never drawn as a Response section the wire never sent"
    );
    assert!(
        cx.debug_bounds(REQUEST_SECTION).is_none(),
        "a notice has no request of its own"
    );
}

#[gpui::test]
async fn wire_entry_expands_under_the_request_and_response_sections(cx: &mut TestAppContext) {
    let (_panel, cx) = expanded_details(wire_entry(), cx);

    assert!(
        cx.debug_bounds(REQUEST_SECTION).is_some(),
        "a wire entry still draws its request section"
    );
    assert!(
        cx.debug_bounds(RESPONSE_SECTION).is_some(),
        "a wire entry still draws its response section"
    );
    assert!(
        cx.debug_bounds(NOTICE_SECTION).is_none(),
        "wire text is never drawn as a notice"
    );
}
