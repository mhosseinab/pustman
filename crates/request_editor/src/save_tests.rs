//! `workspace::Save` (Ctrl-S / Cmd-S, REQ-WSP-01) driven the way a user drives it: the
//! real keystroke from the default keymap, against a temp workspace on disk. Every
//! assertion is an outcome the user can see — the request's file holding the edit with
//! the dirty dot cleared, and a failed save's toast naming the file and the reason.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use api_model::{HttpRequest, ItemId, Request, Workspace as WorkspaceDocument};
use gpui::{Entity, OperatingSystem, TestAppContext, VisualTestContext};
use ui::components::toast::toast_message_selector;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};
use workspace::{Item, Workspace};
use workspace_store::ItemBody;

use super::{RequestFile, install};
use crate::RequestEditor;

/// The request the tests edit and save: a GET named so its file has a stable name.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.name = String::from("Get pets");
    request.url = String::from("https://example.com/ping");
    request
}

/// Write `request` into the workspace at `root` the way the app's workspace wiring finds
/// it: on disk at the placement the store chose, registered in [`install`] with that
/// placement (`workspace_store::Item.path` is `None` until the store writes one).
fn place_request(root: &Path, request: &HttpRequest) -> RequestFile {
    let item = workspace_store::Item {
        body: ItemBody::Request(Request::Http(request.clone())),
        dir: PathBuf::from("collections"),
        path: None,
    };
    let written = workspace_store::save_item(root, &item).expect("the fixture request saves");
    RequestFile {
        root: root.to_path_buf(),
        item: workspace_store::Item {
            path: Some(written),
            ..item
        },
    }
}

/// Flip `root` read-only the way a workspace does: `pustman.json` declaring a
/// `format_version` whose major is newer than this build writes (REQ-WSP-04).
fn declare_newer_format(root: &Path) {
    let document = WorkspaceDocument {
        id: ItemId::new(),
        name: String::from("Read-only workspace"),
        format_version: String::from("2.0"),
        extra: BTreeMap::new(),
    };
    workspace_store::save_item(
        root,
        &workspace_store::Item {
            body: ItemBody::Workspace(document),
            dir: PathBuf::new(),
            path: None,
        },
    )
    .expect("the workspace document saves");
}

/// A workspace window holding `request` as its one tab, on the app's real init order and
/// the real default keymap, with the cursor where a user editing a request starts: the
/// URL field. `files` is what the app's workspace wiring installs.
fn open_editor(
    cx: &mut TestAppContext,
    request: HttpRequest,
    files: Vec<RequestFile>,
) -> (
    Entity<Workspace>,
    Entity<RequestEditor>,
    &mut VisualTestContext,
) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        install(files, cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    let editor = cx.update(|window, cx| {
        let editor = cx.new(|cx| RequestEditor::new(request, window, cx));
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.add_tab_item(editor.clone(), cx));
        });
        editor
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
    });
    draw_window(cx);
    (workspace, editor, cx)
}

/// The `workspace::Save` keystroke on this platform — Cmd on macOS, Ctrl elsewhere,
/// exactly as `assets/keymaps/` binds it.
fn save_keys(cx: &VisualTestContext) -> &'static str {
    let os = cx.read(|cx| cx.platform_info().os);
    if os == OperatingSystem::Mac {
        "cmd-s"
    } else {
        "ctrl-s"
    }
}

#[gpui::test]
fn ctrl_s_saves_and_clears_dirty(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let request = request();
    let file = place_request(directory.path(), &request);
    let written = file
        .item
        .path
        .clone()
        .expect("the fixture request has a file");
    let (_workspace, editor, cx) = open_editor(cx, request, vec![file]);

    // The user edits the URL: that is what turns the tab dirty.
    cx.simulate_keystrokes("end");
    cx.simulate_input("/v2");
    cx.run_until_parked();
    let edited = cx.read(|cx| editor.read_with(cx, |editor, _| editor.draft().url.clone()));
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx))),
        "the edit turns the tab dirty before the save"
    );

    cx.simulate_keystrokes(save_keys(cx));
    cx.run_until_parked();

    let on_disk = std::fs::read_to_string(directory.path().join(&written))
        .expect("the request's file is there");
    let saved: Request = serde_json::from_str(&on_disk).expect("the file is the request's JSON");
    let Request::Http(saved) = saved else {
        panic!("the file holds an HTTP request");
    };
    assert_eq!(
        saved.url, edited,
        "the save writes the edited request to its file"
    );
    assert!(
        !cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx))),
        "a successful save clears the dirty dot"
    );
}

#[gpui::test]
fn save_failure_shows_toast(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let request = request();
    let file = place_request(directory.path(), &request);
    let written = file
        .item
        .path
        .clone()
        .expect("the fixture request has a file");
    declare_newer_format(directory.path());
    let (_workspace, editor, cx) = open_editor(cx, request, vec![file]);

    cx.simulate_keystrokes("end");
    cx.simulate_input("/v2");
    cx.run_until_parked();
    cx.simulate_keystrokes(save_keys(cx));
    cx.run_until_parked();
    draw_window(cx);

    let message = format!(
        "Could not save {} — read-only: pustman.json declares format_version 2.0",
        directory.path().join(&written).display()
    );
    let selector: &'static str = toast_message_selector(&message).leak();
    assert!(
        cx.debug_bounds(selector).is_some(),
        "the failed save is reported in a toast naming the file and the reason"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx))),
        "a failed save leaves the tab dirty"
    );
}
