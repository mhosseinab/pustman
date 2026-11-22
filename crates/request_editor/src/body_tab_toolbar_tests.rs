//! The Body tab's one toolbar row (#238): the mode picker, the language picker and
//! Beautify share a single row — mode and language at the left, Beautify at the right —
//! instead of stacking the pickers on rows of their own over the raw editor (S03/R03).

use api_model::{HttpRequest, Method, RawLanguage};
use gpui::{Bounds, Entity, Focusable, Modifiers, Pixels, TestAppContext, VisualTestContext, px};
use ui::test_support::{draw_window, init_test};

use super::{
    BODY_BEAUTIFY_SELECTOR, BODY_ERROR_SELECTOR, BODY_LANGUAGE_SELECTOR, BODY_MODE_SELECTOR,
    BODY_TOOLBAR_SELECTOR, BodyMode, BodyTab,
};
use crate::RequestEditor;

/// How far two controls' vertical centres may drift and still read as one row.
const SAME_ROW_TOLERANCE: Pixels = px(2.);

/// A draft POST — a body belongs on a method that carries one.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.method = Method::Post;
    request
}

/// A request tab on the app's real init order at the reference window size, with the
/// Body tab open on a raw body in `language` — the state the misaligned-rows
/// screenshot was taken in.
fn open_raw_body(
    cx: &mut TestAppContext,
    language: RawLanguage,
) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request(), window, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.simulate_resize(gpui::size(px(1440.), px(900.)));
    draw_window(cx);

    let tab = cx
        .debug_bounds("request-tab-body")
        .expect("the Body tab is in the request tab strip");
    cx.simulate_click(tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);

    let body_tab = cx.read(|cx| editor.read(cx).body_tab().clone());
    body_tab.update(cx, |tab, cx| {
        tab.set_mode(BodyMode::Raw, cx);
        tab.set_raw_language(language, cx);
    });
    cx.run_until_parked();
    draw_window(cx);
    (editor, cx)
}

/// The painted bounds of one toolbar control.
fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"))
}

/// The vertical centre of a control's painted bounds.
fn center_y(bounds: &Bounds<Pixels>) -> Pixels {
    bounds.origin.y + bounds.size.height / 2.
}

/// Whether anything paints under this debug key (`m1_layout_fixes_tests.rs`'s shape).
fn shown(cx: &mut VisualTestContext, selector: &str) -> bool {
    cx.debug_bounds(selector.to_owned().leak()).is_some()
}

/// Click one control the way a user does.
fn click(selector: &'static str, cx: &mut VisualTestContext) {
    let target = bounds(selector, cx);
    cx.simulate_click(target.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

/// Type `text` into the raw body editor, the way a user does.
fn type_raw(body_tab: &Entity<BodyTab>, cx: &mut VisualTestContext, text: &str) {
    cx.update(|window, cx| {
        let focus_handle = body_tab.read(cx).raw_text_area().read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    });
    draw_window(cx);
    cx.simulate_input(text);
    cx.run_until_parked();
    draw_window(cx);
}

/// The text the raw body editor holds.
fn raw_text(body_tab: &Entity<BodyTab>, cx: &VisualTestContext) -> String {
    cx.read(|cx| body_tab.read(cx).raw_text_area().read(cx).text())
}

#[gpui::test]
fn test_mode_language_and_beautify_share_one_toolbar_row(cx: &mut TestAppContext) {
    let (editor, cx) = open_raw_body(cx, RawLanguage::Json);

    let toolbar = bounds(BODY_TOOLBAR_SELECTOR, cx);
    let mode = bounds(BODY_MODE_SELECTOR, cx);
    let language = bounds(BODY_LANGUAGE_SELECTOR, cx);
    let beautify = bounds(BODY_BEAUTIFY_SELECTOR, cx);

    for (name, control) in [("the language picker", &language), ("Beautify", &beautify)] {
        assert!(
            (center_y(&mode) - center_y(control)).abs() < SAME_ROW_TOLERANCE,
            "{name} sits on the mode picker's row — one toolbar row, not stacked rows: \
             mode {mode:?}, control {control:?}"
        );
        assert!(
            control.origin.y >= toolbar.origin.y
                && control.origin.y + control.size.height <= toolbar.origin.y + toolbar.size.height,
            "{name} is inside the toolbar row: toolbar {toolbar:?}, control {control:?}"
        );
    }

    assert!(
        language.origin.x > mode.origin.x,
        "the language picker follows the mode picker: mode {mode:?}, language {language:?}"
    );
    assert!(
        beautify.origin.x >= language.origin.x + language.size.width,
        "Beautify follows the language picker: language {language:?}, beautify {beautify:?}"
    );
    assert!(
        beautify.origin.x + beautify.size.width
            >= toolbar.origin.x + toolbar.size.width - SAME_ROW_TOLERANCE,
        "Beautify ends the toolbar row at the right: toolbar {toolbar:?}, beautify {beautify:?}"
    );

    // The row is a toolbar *over* the editor: the pickers no longer stack above it.
    let body_tab = cx.read(|cx| editor.read(cx).body_tab().clone());
    let area = cx.read(|cx| body_tab.read(cx).raw_text_area().clone());
    let editor_bounds = cx
        .read(|cx| area.read(cx).painted_bounds())
        .expect("the raw body editor paints");
    assert!(
        editor_bounds.origin.y >= toolbar.origin.y + toolbar.size.height,
        "the raw editor starts under the toolbar row: toolbar {toolbar:?}, editor {editor_bounds:?}"
    );
}

#[gpui::test]
fn test_beautify_reformats_the_raw_json_body(cx: &mut TestAppContext) {
    let (editor, cx) = open_raw_body(cx, RawLanguage::Json);
    let body_tab = cx.read(|cx| editor.read(cx).body_tab().clone());

    type_raw(&body_tab, cx, "{\"a\":1,\"b\":[2,3]}");
    click(BODY_BEAUTIFY_SELECTOR, cx);

    assert_eq!(
        raw_text(&body_tab, cx),
        "{\n  \"a\": 1,\n  \"b\": [\n    2,\n    3\n  ]\n}",
        "Beautify re-formats the raw body with even indentation"
    );
    assert!(
        !shown(cx, BODY_ERROR_SELECTOR),
        "a body that formats leaves no note behind"
    );
}

#[gpui::test]
fn test_beautify_keeps_an_unparseable_body_and_says_why(cx: &mut TestAppContext) {
    let (editor, cx) = open_raw_body(cx, RawLanguage::Json);
    let body_tab = cx.read(|cx| editor.read(cx).body_tab().clone());

    type_raw(&body_tab, cx, "{\"a\":");
    click(BODY_BEAUTIFY_SELECTOR, cx);

    assert_eq!(
        raw_text(&body_tab, cx),
        "{\"a\":",
        "a body Beautify cannot parse is left exactly as it is"
    );
    assert!(
        shown(cx, BODY_ERROR_SELECTOR),
        "Beautify says why it could not format the body — never a silent no-op"
    );
}

#[gpui::test]
fn test_beautify_on_a_text_body_says_what_it_needs(cx: &mut TestAppContext) {
    let (editor, cx) = open_raw_body(cx, RawLanguage::Text);
    let body_tab = cx.read(|cx| editor.read(cx).body_tab().clone());

    type_raw(&body_tab, cx, "hello");
    click(BODY_BEAUTIFY_SELECTOR, cx);

    assert_eq!(
        raw_text(&body_tab, cx),
        "hello",
        "a text body is left exactly as it is"
    );
    assert!(
        shown(cx, BODY_ERROR_SELECTOR),
        "Beautify names the languages it can format instead of doing nothing"
    );
}
