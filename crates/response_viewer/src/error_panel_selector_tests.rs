//! The error panel's debug keys name the text its labels actually render (#125): the
//! fix line reads "Suggested fix: {fix}", so its key must derive from that composed
//! label. A key derived from the bare state keeps tests green while the label drifts
//! away from the claim they make — finding the key must prove the wording on screen.

use gpui::{TestAppContext, VisualTestContext};
use http_engine::TransportError;
use ui::test_support::{draw_window, init_test};

use super::{error_fix_selector, error_message_selector};
use crate::ResponseView;

#[gpui::test]
fn fix_line_debug_key_derives_from_the_rendered_label_text(cx: &mut TestAppContext) {
    cx.update(init_test);
    let (view, cx) = cx.add_window_view(|_, cx| ResponseView::new(cx));
    let error = TransportError::Dns {
        host: "api.example.test".to_owned(),
    };
    view.update(cx, |view, cx| view.set_error(error.clone(), cx));
    draw_window(cx);

    // The label the panel renders is the composed text; the key names exactly it.
    let fix_label = format!("Suggested fix: {}", error.suggested_fix());
    assert_eq!(
        error_fix_selector(error.suggested_fix()),
        format!("error-panel-fix-{fix_label}"),
        "the fix key derives from the rendered label, not the bare state"
    );
    assert!(
        shown(cx, format!("error-panel-fix-{fix_label}")),
        "the fix line renders under the key its rendered label derives"
    );

    // The message renders verbatim; its key names that same text.
    let message = error.user_message();
    assert!(
        shown(cx, error_message_selector(&message)),
        "the message renders under the key its rendered text derives"
    );
}

/// Whether anything is on screen under this debug key (the `error_panel_tests.rs`
/// pattern: `VisualTestContext::debug_bounds` takes a `&'static str`, so the selector is
/// leaked — fine for a test's short life).
fn shown(cx: &mut VisualTestContext, selector: String) -> bool {
    cx.debug_bounds(selector.leak()).is_some()
}
