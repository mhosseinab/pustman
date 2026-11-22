//! The cURL import's undo row is a compact info banner with one small secondary action
//! (#237) — not tiny muted text beside a large outlined button. What GPUI's test
//! support exposes is the layout (`debug_bounds`) and the painted quads, which is what
//! these tests speak: the banner draws in the Info severity's tint, its action is a
//! small button (a frame at most 24 px tall) and it is secondary — it draws no control
//! outline.
//!
//! `Decision:` the two `banner-*-` selector strings are spelled out here rather than
//! imported from `ui::components::banner`, which carries
//! [`banner_message_selector`](ui::components::banner::banner_message_selector) and
//! [`banner_action_selector`](ui::components::banner::banner_action_selector) as the
//! source of truth: the tests state the contract, and a rename of the keys must break
//! them loudly.

use gpui::{ClipboardItem, TestAppContext, VisualTestContext, px};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};

use crate::RequestEditor;

/// The message the undo banner states — the anchor its test keys are derived from.
const UNDO_MESSAGE: &str = "Imported a cURL command";

/// `ui::components::banner::banner_message_selector(UNDO_MESSAGE)`.
const MESSAGE_SELECTOR: &str = "banner-message-Imported a cURL command";

/// `ui::components::banner::banner_action_selector(UNDO_MESSAGE)`.
const ACTION_SELECTOR: &str = "banner-action-Imported a cURL command";

/// A cURL command simple enough to import and to leave the undo banner behind.
const CURL_COMMAND: &str = "curl https://example.com/ping";

/// An editor with `CURL_COMMAND` imported — the state the undo banner shows in.
fn imported(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| {
        RequestEditor::new(crate::new_request::fresh_request(), window, cx)
    });
    cx.update(|window, _cx| window.activate_window());
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
    });
    draw_window(cx);
    cx.write_to_clipboard(ClipboardItem::new_string(String::from(CURL_COMMAND)));
    cx.simulate_keystrokes(paste_keys(cx));
    cx.run_until_parked();
    draw_window(cx);
    cx
}

/// The `text_input::Paste` keystroke on this platform — Cmd on macOS, Ctrl elsewhere.
fn paste_keys(cx: &VisualTestContext) -> &'static str {
    let os = cx.read(|cx| cx.platform_info().os);
    if os == gpui::OperatingSystem::Mac {
        "cmd-v"
    } else {
        "ctrl-v"
    }
}

#[gpui::test]
fn the_undo_row_draws_as_an_info_banner(cx: &mut TestAppContext) {
    let cx = imported(cx);
    let (info_background, info_border) =
        cx.read(|cx| (Severity::Info.background(cx), Severity::Info.border(cx)));
    let quads = cx.update(|window, _cx| window.painted_quads());

    assert!(
        cx.debug_bounds(MESSAGE_SELECTOR).is_some(),
        "the banner states its message: {UNDO_MESSAGE}"
    );
    assert!(
        quads
            .iter()
            .any(|quad| quad.background.as_solid() == Some(info_background)),
        "the undo row fills with the Info severity's background — a banner around the \
         message, not bare muted text"
    );
    assert!(
        quads.iter().any(|quad| quad.border_color == info_border),
        "and it is outlined in the Info severity's border"
    );
}

#[gpui::test]
fn the_undo_action_is_a_small_secondary_button(cx: &mut TestAppContext) {
    let cx = imported(cx);
    let action = cx
        .debug_bounds(ACTION_SELECTOR)
        .expect("the undo banner offers its action under the banner's key");
    assert!(
        action.size.height <= px(24.),
        "the undo action is a small button, not the ButtonSize::Medium one the row \
         used to draw: {:?} tall",
        action.size.height
    );

    let control_outline = cx.read(|cx| cx.theme().colors().border);
    let quads = cx.update(|window, _cx| window.painted_quads());
    let scale = cx.update(|window, _cx| window.scale_factor());
    let frame = action.scale(scale);
    assert!(
        quads
            .iter()
            .filter(|quad| quad.bounds.intersects(&frame))
            .all(|quad| quad.border_color != control_outline),
        "the undo action is secondary: it draws no control outline"
    );
}
