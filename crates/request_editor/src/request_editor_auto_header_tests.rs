//! The auto-header switches reaching the request draft (REQ-HTTP-03, S27d fix round
//! 1): the disabled-auto-header list the Headers tab edits is the list the request
//! draft carries — what send and save are built from — and a draft replaced wholesale
//! re-seeds the tab's switches from the new draft, so a stale toggle list never rides
//! onto an imported request.

use std::collections::BTreeMap;

use api_model::{Body, HttpRequest, Method, RawLanguage};
use gpui::{ClipboardItem, Entity, Modifiers, OperatingSystem, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};
use workspace::Item;

use crate::RequestEditor;

/// A draft POST with a JSON body — a request whose auto headers (User-Agent among
/// them) the send pipeline lists.
fn request_with_json_body() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.method = Method::Post;
    request.url = String::from("https://example.com/items");
    request.body = Body::Raw {
        language: RawLanguage::Json,
        text: String::from("{\"name\":\"pustman\"}"),
        extra: BTreeMap::new(),
    };
    request
}

/// A request tab on the app's real init order and the real default keymap, so the
/// paste keystroke below is the one the app binds.
fn open_editor(
    cx: &mut TestAppContext,
    request: HttpRequest,
) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (editor, cx)
}

/// Open the Headers tab the way a user does: click it in the request tab strip.
fn open_headers_tab(cx: &mut VisualTestContext) {
    let tab = cx
        .debug_bounds("request-tab-headers")
        .expect("the Headers tab is in the request tab strip");
    cx.simulate_click(tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

/// Click one auto row's send switch, the way a user does.
fn click_auto_toggle(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx
        .debug_bounds(selector)
        .expect("the auto row's switch renders");
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

/// The disabled-auto-header names the request draft carries — what a send and a save
/// are built from (`RequestEditor::draft`).
fn draft_disabled_auto_headers(
    editor: &Entity<RequestEditor>,
    cx: &VisualTestContext,
) -> Vec<String> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, _| {
            editor
                .draft()
                .settings
                .as_ref()
                .map_or_else(Vec::new, |settings| settings.disabled_auto_headers.clone())
        })
    })
}

/// Whether the Headers tab switches `name` off — the state its send switch renders
/// (`Headers::draft_settings`).
fn tab_disables(editor: &Entity<RequestEditor>, cx: &VisualTestContext, name: &str) -> bool {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor
                .headers()
                .read(cx)
                .draft_settings()
                .is_auto_header_disabled(name)
        })
    })
}

/// The `text_input::Paste` keystroke on this platform — Cmd on macOS, Ctrl elsewhere,
/// exactly as `text_input::init` binds it.
fn paste_keys(cx: &VisualTestContext) -> &'static str {
    let os = cx.read(|cx| cx.platform_info().os);
    if os == OperatingSystem::Mac {
        "cmd-v"
    } else {
        "ctrl-v"
    }
}

#[gpui::test]
fn toggling_an_auto_header_off_writes_the_disabled_list_into_the_request_draft(
    cx: &mut TestAppContext,
) {
    let (editor, cx) = open_editor(cx, request_with_json_body());
    open_headers_tab(cx);
    assert!(
        draft_disabled_auto_headers(&editor, cx).is_empty(),
        "the request starts with every auto header sent"
    );

    // Switch User-Agent off at its row's control, the way a user does.
    click_auto_toggle("auto-toggle-User-Agent", cx);
    assert_eq!(
        draft_disabled_auto_headers(&editor, cx),
        vec![String::from("User-Agent")],
        "switching the row off writes the name into the request draft's settings (REQ-HTTP-03)"
    );

    // Switching it back on takes the name out again — and an edit and its undo leave
    // the tab clean, as `RequestEditor::saved` promises.
    click_auto_toggle("auto-toggle-User-Agent", cx);
    assert!(
        draft_disabled_auto_headers(&editor, cx).is_empty(),
        "switching the row back on leaves nothing disabled in the draft"
    );
    let dirty = cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx)));
    assert!(!dirty, "the toggle's undo leaves the tab clean");
}

#[gpui::test]
fn replacing_the_draft_reseeds_the_disabled_auto_headers(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, request_with_json_body());
    open_headers_tab(cx);

    // The setup: the draft leaves User-Agent out of the send.
    click_auto_toggle("auto-toggle-User-Agent", cx);
    assert_eq!(
        draft_disabled_auto_headers(&editor, cx),
        vec![String::from("User-Agent")],
        "the setup switches User-Agent off for the draft"
    );

    // Replace the draft wholesale — the cURL import path — with a request whose
    // settings disable nothing.
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
    });
    draw_window(cx);
    cx.write_to_clipboard(ClipboardItem::new_string(
        "curl 'https://example.com/health'".to_string(),
    ));
    cx.simulate_keystrokes(paste_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    let url = cx.read(|cx| editor.read(cx).draft().url.clone());
    assert_eq!(
        url, "https://example.com/health",
        "the paste replaced the draft with the imported request"
    );

    assert!(
        cx.debug_bounds("auto-toggle-User-Agent").is_some(),
        "the imported request still lists its User-Agent auto row"
    );
    assert!(
        !tab_disables(&editor, cx, "User-Agent"),
        "the tab re-read the new draft: the User-Agent switch is on again"
    );
    assert!(
        draft_disabled_auto_headers(&editor, cx).is_empty(),
        "and the imported request's draft disables nothing"
    );
}
