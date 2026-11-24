//! Typing is refused in a read-only workspace (REQ-WSP-04): the URL bar of a request tab
//! leaves the draft exactly as it was while the workspace opened on a newer major
//! `format_version` (S41c), and takes the same edit again once the workspace is writable
//! — the flag follows the open workspace, so a tab cannot out-edit it. Driven the way a
//! user drives it: the real `OpenWorkspace` action and its folder dialog, then a real
//! edit through the focused URL field.

use std::path::Path;

use api_model::{HttpRequest, ItemId};
use gpui::{AppContext, Entity, TestAppContext, VisualTestContext};
use pustman_actions::workspace::OpenWorkspace;
use ui::test_support::{draw_window, init_test};
use workspace::Workspace;
use workspace::read_only::is_read_only;

use crate::RequestEditor;

/// The request the test types into: a GET with a URL the edit can extend.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.com/ping");
    request
}

/// A workspace folder whose `pustman.json` declares `format_version` — the fact S41c
/// derives read-only-ness from.
fn workspace_file(root: &Path, format_version: &str) {
    let document = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "Fixture workspace",
        "format_version": format_version,
    });
    let bytes = serde_json::to_string_pretty(&document).expect("the fixture serializes");
    std::fs::write(root.join("pustman.json"), bytes).expect("the fixture file is written");
}

/// The workspace window on the app's real init order, with the request editor wired.
fn workspace_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        workspace::init(cx);
        crate::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    (workspace, cx)
}

/// Open `root` as this window's workspace: the real `OpenWorkspace` action and its
/// folder dialog (the `read_only_tests.rs` shape).
fn open_workspace_folder(cx: &mut VisualTestContext, root: &Path) {
    cx.dispatch_action(OpenWorkspace);
    cx.run_until_parked();
    assert!(
        cx.did_prompt_for_paths(),
        "OpenWorkspace asks for the workspace folder in a dialog"
    );
    cx.simulate_path_prompt_response(|_| Some(vec![root.to_path_buf()]));
    cx.run_until_parked();
}

/// One request tab with the cursor in its URL field, the way a user starts editing.
fn open_request_tab(
    cx: &mut VisualTestContext,
    workspace: &Entity<Workspace>,
) -> Entity<RequestEditor> {
    let editor = cx.update(|window, cx| {
        let editor = cx.new(|cx| RequestEditor::new(request(), window, cx));
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.add_tab_item(editor.clone(), cx));
        });
        editor
    });
    focus_url_bar(&editor, cx);
    editor
}

/// Put the cursor in the tab's URL field and paint, so keystrokes reach it.
fn focus_url_bar(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
    });
    draw_window(cx);
}

/// The draft's URL right now — what a rejected edit must leave alone.
fn draft_url(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> String {
    cx.read(|cx| editor.read_with(cx, |editor, _| editor.draft().url.clone()))
}

/// Type `text` at the end of the URL field, the way a user extends the URL.
fn type_at_url_end(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_keystrokes("end");
    cx.simulate_input(text);
    cx.run_until_parked();
}

#[gpui::test]
fn read_only_workspace_request_inputs_reject_typing(cx: &mut TestAppContext) {
    let read_only_root = tempfile::tempdir().expect("the read-only workspace folder opens");
    workspace_file(read_only_root.path(), "2.0");
    let writable_root = tempfile::tempdir().expect("the writable workspace folder opens");
    workspace_file(writable_root.path(), "1.0");
    let (workspace, cx) = workspace_window(cx);

    // The workspace opens read-only (S41c): a newer major `format_version` than this
    // build writes.
    open_workspace_folder(cx, read_only_root.path());
    assert!(
        cx.read(is_read_only),
        "the fixture workspace opens read-only"
    );

    let editor = open_request_tab(cx, &workspace);
    let before = draft_url(&editor, cx);
    type_at_url_end(cx, "/v2");
    assert_eq!(
        draft_url(&editor, cx),
        before,
        "typing into the URL bar of a read-only workspace leaves the draft unchanged"
    );

    // A writable workspace takes the edit again: the same tab follows the open workspace.
    open_workspace_folder(cx, writable_root.path());
    assert!(
        !cx.read(is_read_only),
        "a writable workspace clears the read-only flag"
    );
    focus_url_bar(&editor, cx);
    type_at_url_end(cx, "/v2");
    assert_eq!(
        draft_url(&editor, cx),
        format!("{before}/v2"),
        "a writable workspace still accepts the edit"
    );
}
