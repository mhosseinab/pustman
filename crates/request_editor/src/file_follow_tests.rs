//! The tab follows its file (#308): a rename behind the app's back moves the save
//! target with it — the old-name file is never written back — and a delete flags the
//! tab and drops its save target instead of resurrecting the file as an orphan. The
//! same real open path as the bar's tests: [`WorkspaceOpen::open_folder`] and the real
//! watcher. Wired from `request_editor.rs` (not `disk_change.rs`) so the pinned
//! `disk_change::` filter still reports its three tests.

use std::path::{Path, PathBuf};
use std::time::Duration;

use api_model::{HttpRequest, ItemId, Request};
use gpui::{Entity, OperatingSystem, TestAppContext, VisualTestContext};
use tempfile::TempDir;
use ui::components::toast::toast_message_selector;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};
use workspace::{Item, Workspace, WorkspaceOpen};
use workspace_store::{Item as StoreItem, ItemBody};

use crate::RequestEditor;
use crate::disk_change::FILE_GONE_SELECTOR;
use crate::save::{self, RequestFile, install};

/// The request the tests open: a GET whose file has a stable name.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.name = String::from("Get pets");
    request.url = String::from("https://example.com/pets");
    request
}

/// Write `root`'s workspace the way the app opens it (PRD §6.1): `pustman.json` and the
/// request's file, returned at the placement a save finds it.
fn fixture_workspace(root: &Path, request: &HttpRequest) -> RequestFile {
    let document = serde_json::json!({
        "id": ItemId::new().to_string(),
        "name": "File follow workspace",
        "format_version": "1.0",
    });
    let bytes = serde_json::to_string_pretty(&document).expect("the workspace file serializes");
    std::fs::write(root.join("pustman.json"), bytes).expect("the workspace file is written");
    let item = StoreItem {
        body: ItemBody::Request(Request::Http(request.clone())),
        dir: PathBuf::from("collections"),
        path: None,
    };
    let written = workspace_store::save_item(root, &item).expect("the fixture request saves");
    RequestFile {
        root: root.to_path_buf(),
        item: StoreItem {
            path: Some(written),
            ..item
        },
    }
}

/// A workspace window holding `request` as its one tab, opened through the real open
/// path: [`WorkspaceOpen::open_folder`] loads the snapshot and this tab's watcher
/// starts over the folder exactly as in the running app.
fn open_tab(
    cx: &mut TestAppContext,
    request: HttpRequest,
    file: RequestFile,
) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    let root = file.root.clone();
    cx.update(init_test);
    cx.update(|cx| {
        workspace::init(cx);
        text_input::init(cx);
        crate::init(cx);
        install([file], cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    cx.update(|_, cx| {
        let opener = WorkspaceOpen::for_window(workspace.entity_id(), cx)
            .expect("every workspace window has its WorkspaceOpen");
        opener.update(cx, |opener, cx| opener.open_folder(root, cx));
    });
    cx.run_until_parked();
    let editor = cx.update(|window, cx| {
        let editor = cx.new(|cx| RequestEditor::new(request, window, cx));
        workspace.update(cx, |workspace, cx| {
            workspace
                .center_pane()
                .update(cx, |pane, cx| pane.add_tab_item(editor.clone(), cx));
        });
        editor
    });
    cx.update(|window, cx| editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx)));
    cx.run_until_parked();
    (editor, cx)
}

/// Bounded wait for a tab title: the watcher is a real `notify` thread over a real
/// folder, so this passes real time but gives up after ~2 s instead of hanging.
async fn wait_for_title(
    editor: &Entity<RequestEditor>,
    title: &str,
    cx: &mut VisualTestContext,
) -> bool {
    for _ in 0..20 {
        if cx.read(|cx| editor.read_with(cx, |editor, cx| editor.tab_title(cx).as_ref() == title)) {
            return true;
        }
        cx.executor().timer(Duration::from_millis(100)).await;
    }
    false
}

/// Bounded wait for the tab's "file gone" flag (same shape as [`wait_for_title`]).
async fn wait_for_flag(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) -> bool {
    for _ in 0..20 {
        if cx.read(|cx| editor.read_with(cx, |editor, _| editor.file_gone().is_some())) {
            return true;
        }
        cx.executor().timer(Duration::from_millis(100)).await;
    }
    false
}

/// The `workspace::Save` keystroke on this platform — Cmd on macOS, Ctrl elsewhere.
fn save_keys(cx: &VisualTestContext) -> &'static str {
    let os = cx.read(|cx| cx.platform_info().os);
    if os == OperatingSystem::Mac {
        "cmd-s"
    } else {
        "ctrl-s"
    }
}

#[gpui::test]
async fn rename_follows_the_tabs_file(cx: &mut TestAppContext) {
    // The rename under test is real IO over a real tempdir: parking lets real time pass
    // through the GPUI timers the waits below use.
    cx.executor().allow_parking();
    let temp = TempDir::new().expect("a scratch workspace folder");
    let request = request();
    let file = fixture_workspace(temp.path(), &request);
    let old_path = file
        .item
        .path
        .clone()
        .expect("the fixture request has a file");
    let (editor, cx) = open_tab(cx, request.clone(), file.clone());

    // The rename behind the app's back — the collections panel's rename writes the new
    // name and takes the old file with it (`workspace_store::save_item` does the move).
    let mut renamed = file.item.clone();
    renamed.set_name("Get pets renamed");
    let new_path =
        workspace_store::save_item(temp.path(), &renamed).expect("the renamed request saves");

    assert!(
        wait_for_title(&editor, "Get pets renamed", cx).await,
        "a clean tab follows the renamed file"
    );
    assert_eq!(
        cx.read(|cx| save::file_for(&request.id, cx).and_then(|file| file.item.path)),
        Some(new_path.clone()),
        "the save target follows the file"
    );

    // The next save writes where the request lives now — and never the old name back.
    cx.simulate_keystrokes("end");
    cx.simulate_input("/v2");
    cx.run_until_parked();
    let edited_url = cx.read(|cx| editor.read_with(cx, |editor, _| editor.draft().url.clone()));
    cx.simulate_keystrokes(save_keys(cx));
    cx.run_until_parked();

    assert!(
        !temp.path().join(&old_path).exists(),
        "the save never writes the old-name file back (two files, one id)"
    );
    let on_disk =
        std::fs::read_to_string(temp.path().join(&new_path)).expect("the renamed file is there");
    let saved: Request = serde_json::from_str(&on_disk).expect("the file is the request's JSON");
    let Request::Http(saved) = saved else {
        panic!("the file holds an HTTP request");
    };
    assert_eq!(saved.url, edited_url, "the save lands in the renamed file");
}

#[gpui::test]
async fn delete_flags_the_tab_and_forgets_its_file(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let temp = TempDir::new().expect("a scratch workspace folder");
    let request = request();
    let file = fixture_workspace(temp.path(), &request);
    let written = file
        .item
        .path
        .clone()
        .expect("the fixture request has a file");
    let (editor, cx) = open_tab(cx, request.clone(), file.clone());

    // The user is mid-edit when the file goes: the draft must survive.
    cx.simulate_keystrokes("end");
    cx.simulate_input("/v2");
    cx.run_until_parked();

    // The delete behind the app's back.
    std::fs::remove_file(temp.path().join(&written)).expect("the request's file is deleted");

    assert!(
        wait_for_flag(&editor, cx).await,
        "a deleted file flags the tab within the 1 s budget"
    );
    draw_window(cx);
    assert!(
        cx.debug_bounds(FILE_GONE_SELECTOR).is_some(),
        "the tab states that its file is gone"
    );

    // The next save must not bring the file back as an orphan (#308).
    cx.simulate_keystrokes(save_keys(cx));
    cx.run_until_parked();

    assert!(
        !temp.path().join(&written).exists(),
        "the save does not resurrect the deleted file"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx))),
        "the draft is kept"
    );
    assert!(
        cx.read(|cx| save::file_for(&request.id, cx)).is_none(),
        "the tab no longer claims a file to save to"
    );
    let message = format!(
        "Could not save {} — it has no file in a workspace yet",
        request.name
    );
    let selector: &'static str = toast_message_selector(&message).leak();
    assert!(
        cx.debug_bounds(selector).is_some(),
        "the refused save explains itself where the user is looking"
    );
}
