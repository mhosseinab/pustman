//! The Path Variables section exists only when the URL names path variables (#221):
//! Postman shows the section for `:name` segments and only then, so a URL without them
//! keeps the tab to Query Params alone. Driven the way a user drives it — typing the
//! URL — and asserted on what paints (the section's debug key).

use gpui::{Entity, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use crate::RequestEditor;
use crate::params::PATH_SECTION_SELECTOR;

/// A draft GET to `url`, the way a request tab opens.
fn request(url: &str) -> api_model::HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from(url);
    request
}

/// A request tab on the app's real init order, with the cursor where a user starts: the
/// URL field.
fn open_editor(
    cx: &mut TestAppContext,
    request: api_model::HttpRequest,
) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.update(|window, cx| editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx)));
    draw_window(cx);
    (editor, cx)
}

/// Replace the URL field's text the way a user does: select all, then type.
fn type_url(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext, url: &str) {
    cx.update(|window, cx| editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx)));
    let select_all = if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    };
    cx.simulate_keystrokes(select_all);
    cx.simulate_input(url);
    cx.run_until_parked();
    draw_window(cx);
}

/// Whether anything paints under `selector`.
fn shown(selector: &'static str, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(selector).is_some()
}

#[gpui::test]
fn test_no_path_variables_section_without_path_segments(cx: &mut TestAppContext) {
    let (_editor, cx) = open_editor(cx, request("https://example.com/ping"));

    assert!(
        shown(crate::REQUEST_TAB_CONTENT_SELECTOR, cx),
        "the Params tab paints its content — the check below is not vacuous"
    );
    assert!(
        !shown(PATH_SECTION_SELECTOR, cx),
        "a URL with no :name segments shows no Path Variables section"
    );
}

#[gpui::test]
fn test_path_variables_appear_when_the_url_names_a_path_segment(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, request("https://example.com/ping"));
    type_url(&editor, cx, "https://example.com/users/:id");

    assert!(
        shown(PATH_SECTION_SELECTOR, cx),
        "a URL with a :name segment shows the Path Variables section"
    );
}

#[gpui::test]
fn test_path_variables_disappear_when_the_url_loses_its_path_segment(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, request("https://example.com/users/:id"));
    assert!(
        shown(PATH_SECTION_SELECTOR, cx),
        "the URL opens with a :name segment, so the section starts out shown"
    );
    type_url(&editor, cx, "https://example.com/users/42");

    assert!(
        !shown(PATH_SECTION_SELECTOR, cx),
        "a URL whose :name segment is gone shows no Path Variables section"
    );
}
