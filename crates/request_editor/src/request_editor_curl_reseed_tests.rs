//! The cURL import's re-seed of the Body and Auth tabs — the review finding S28b
//! closes (S28a left both tabs holding the state of the request the import replaced,
//! so the next tab edit overwrote the imported body). Pasting the real `Paste`
//! keystroke imports a command with a body and a credential; the tabs must show the
//! imported values and the next body edit must extend the imported body, not lose it.

use std::collections::BTreeMap;

use api_model::{Auth, Body, RawLanguage};
use gpui::{
    ClipboardItem, Entity, Focusable, Modifiers, OperatingSystem, TestAppContext, VisualTestContext,
};
use ui::test_support::{draw_window, init_test};

use crate::RequestEditor;

/// The cURL command the import test pastes: a POST with a JSON body and a Basic
/// credential — everything the Body and Auth tabs must show afterwards.
const CURL_COMMAND: &str = "curl -X POST 'https://api.example.com/echo' \
     -H 'Content-Type: application/json' \
     -d 'imported-body' \
     -u 'api-user:token-REDACTED-example'";

/// The body the command's `-d` imports: raw text, typed by the Content-Type.
const IMPORTED_BODY: &str = "imported-body";

/// The edit a user makes after the import, in the body the import brought.
const TYPED_SUFFIX: &str = "-typed";

/// A request tab on the app's real init order and the real default keymap, with the
/// cursor where a paste happens: the URL field.
fn open_editor(cx: &mut TestAppContext) -> (Entity<RequestEditor>, &mut VisualTestContext) {
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
    let (editor, cx) = cx.add_window_view(|window, cx| {
        RequestEditor::new(crate::new_request::fresh_request(), window, cx)
    });
    cx.update(|window, _cx| window.activate_window());
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
    });
    draw_window(cx);
    (editor, cx)
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

/// Open the Body tab the way a user does: click it in the request tab strip.
fn open_body_tab(cx: &mut VisualTestContext) {
    let tab = cx
        .debug_bounds("request-tab-body")
        .expect("the Body tab is in the request tab strip");
    cx.simulate_click(tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

#[gpui::test]
fn curl_import_reseeds_body_and_auth_tabs(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx);

    cx.write_to_clipboard(ClipboardItem::new_string(CURL_COMMAND.to_string()));
    cx.simulate_keystrokes(paste_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    // Both tabs show what the import brought — not the state of the request it
    // replaced.
    let (body_tab, auth_tab) = cx.read(|cx| {
        (
            editor.read(cx).body_tab().clone(),
            editor.read(cx).auth_tab().clone(),
        )
    });
    let imported_body = Body::Raw {
        language: RawLanguage::Json,
        text: String::from(IMPORTED_BODY),
        extra: BTreeMap::new(),
    };
    let shown_body = cx.read(|cx| body_tab.read(cx).body(cx));
    assert_eq!(
        shown_body, imported_body,
        "the Body tab shows the imported body: {shown_body:?}"
    );
    let imported_auth = Auth::Basic {
        username: Some(String::from("api-user")),
        password: Some(String::from("token-REDACTED-example")),
        extra: BTreeMap::new(),
    };
    let shown_auth = cx.read(|cx| auth_tab.read(cx).auth());
    assert_eq!(
        shown_auth, imported_auth,
        "the Auth tab shows the imported credential: {shown_auth:?}"
    );

    // The next body edit extends the imported body instead of overwriting it with
    // the stale state the tab held before the import.
    open_body_tab(cx);
    cx.update(|window, cx| {
        let focus_handle = body_tab.read(cx).raw_text_area().read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    });
    draw_window(cx);
    cx.simulate_input(TYPED_SUFFIX);
    cx.run_until_parked();
    draw_window(cx);

    let edited = cx.read(|cx| editor.read(cx).draft().body.clone());
    let Body::Raw { text, .. } = &edited else {
        panic!("the draft still holds the imported raw body: {edited:?}");
    };
    assert!(
        text.contains(IMPORTED_BODY) && text.contains(TYPED_SUFFIX),
        "the tab edit kept the imported body and added to it: {text:?}"
    );
}
