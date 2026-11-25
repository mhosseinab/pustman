//! Fix-cycle 1's behaviors for the changed-on-disk reaction (REQ-COLLAB-01/02): a
//! conflict on the very first change, a silent clean-tab reload that never takes the
//! keyboard, a "Keep mine" that stays quiet until the file changes again, a file that
//! already holds the draft, and the echo of the tab's own save. Same real open path as
//! the bar's tests: [`WorkspaceOpen::open_folder`] and the real watcher. Wired from
//! `request_editor.rs` (not `disk_change.rs`) so the pinned `disk_change::` filter still
//! reports its three tests.

use std::path::{Path, PathBuf};
use std::time::Duration;

use api_model::{HttpRequest, ItemId, Request};
use gpui::{App, Entity, Focusable, Modifiers, OperatingSystem, TestAppContext, VisualTestContext};
use tempfile::TempDir;
use ui::components::toast::toast_message_selector;
use ui::prelude::*;
use ui::test_support::{draw_window, init_test};
use workspace::{Item, Workspace, WorkspaceOpen};
use workspace_store::{Item as StoreItem, ItemBody};

use crate::RequestEditor;
use crate::disk_change::{FILE_GONE_SELECTOR, KEEP_MINE_SELECTOR, RELOAD_SELECTOR};
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
        "name": "Fix cycle workspace",
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

/// Rewrite the request's file behind the app's back — the `git pull` case.
fn rewrite_file(root: &Path, file: &RequestFile, request: &HttpRequest) {
    let path = root.join(
        file.item
            .path
            .as_deref()
            .expect("the fixture request has a file"),
    );
    let bytes = serde_json::to_string_pretty(&Request::Http(request.clone()))
        .expect("the request serializes");
    std::fs::write(path, bytes).expect("the file is rewritten");
}

/// A workspace window holding `request` as its one tab, opened through the real open
/// path: [`WorkspaceOpen::open_folder`] loads the snapshot and this tab's watcher
/// starts over the folder exactly as in the running app.
fn open_tab(
    cx: &mut TestAppContext,
    request: HttpRequest,
    file: RequestFile,
) -> (
    Entity<Workspace>,
    Entity<RequestEditor>,
    &mut VisualTestContext,
) {
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
    (workspace, editor, cx)
}

/// The user's edit: type onto the URL, which turns the tab dirty.
fn edit_url(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("end");
    cx.simulate_input("/v2");
    cx.run_until_parked();
}

/// A short real-time settle: long enough for a change batch to be polled and applied,
/// for the "nothing happened" assertions (the watcher's 100 ms debounce + 100 ms poll).
async fn settle(cx: &mut VisualTestContext) {
    for _ in 0..5 {
        cx.executor().timer(Duration::from_millis(100)).await;
    }
}

/// Bounded wait for the tab's "file gone" flag: the watcher is a real `notify` thread
/// over a real folder, so this passes real time but gives up after ~2 s.
async fn wait_for_flag(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) -> bool {
    for _ in 0..20 {
        if cx.read(|cx| editor.read_with(cx, |editor, _| editor.file_gone().is_some())) {
            return true;
        }
        cx.executor().timer(Duration::from_millis(100)).await;
    }
    false
}

/// Bounded wait for a predicate on the tab (`wait_for_flag`'s shape).
async fn wait_for_tab(
    editor: &Entity<RequestEditor>,
    cx: &mut VisualTestContext,
    want: impl Fn(&RequestEditor, &App) -> bool,
) -> bool {
    for _ in 0..20 {
        if cx.read(|cx| editor.read_with(cx, |editor, cx| want(editor, cx))) {
            return true;
        }
        cx.executor().timer(Duration::from_millis(100)).await;
    }
    false
}

/// Click one control of the bar the way a user does.
fn click_control(selector: &'static str, cx: &mut VisualTestContext) {
    let bounds = cx.debug_bounds(selector).expect("the control is on screen");
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
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

/// Whether the URL field holds the keyboard right now.
fn url_field_focused(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        editor.read_with(cx, |editor, cx| {
            editor
                .url_bar()
                .read(cx)
                .url_input()
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
        })
    })
}

#[gpui::test]
async fn conflict_markers_on_the_first_change_flag_the_tab_and_block_the_save(
    cx: &mut TestAppContext,
) {
    // The watcher under test is real IO over a real tempdir: parking lets real time
    // pass through the GPUI timers the waits below use.
    cx.executor().allow_parking();
    let temp = TempDir::new().expect("a scratch workspace folder");
    let request = request();
    let file = fixture_workspace(temp.path(), &request);
    let written = file
        .item
        .path
        .clone()
        .expect("the fixture request has a file");
    let (_workspace, editor, cx) = open_tab(cx, request.clone(), file.clone());

    // The user is mid-edit when the `git pull` lands its conflict — and the conflict is
    // the FIRST change this tab's watcher ever sees (REQ-COLLAB-02).
    edit_url(cx);
    std::fs::write(
        temp.path().join(&written),
        "<<<<<<< HEAD\n{}\n=======\n{}\n>>>>>>> teammate\n",
    )
    .expect("the conflicted file is written");

    assert!(
        wait_for_flag(&editor, cx).await,
        "the first change is never dropped: the tab is flagged"
    );
    draw_window(cx);
    assert!(
        cx.debug_bounds(FILE_GONE_SELECTOR).is_some(),
        "the tab states that its file cannot be read"
    );
    assert!(
        cx.read(|cx| {
            editor.read_with(cx, |editor, _| {
                editor
                    .file_gone()
                    .unwrap_or_default()
                    .contains("could not be read")
            })
        }),
        "the banner names the unreadable file"
    );

    // Ctrl-S must refuse: nothing is written over a merge conflict.
    cx.simulate_keystrokes(save_keys(cx));
    cx.run_until_parked();

    let on_disk = std::fs::read_to_string(temp.path().join(&written))
        .expect("the conflicted file is still there");
    assert!(
        on_disk.contains(">>>>>>> teammate"),
        "the save never overwrites the conflict"
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

#[gpui::test]
async fn a_clean_tab_reloads_silently_without_taking_focus(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let temp = TempDir::new().expect("a scratch workspace folder");
    let request = request();
    let file = fixture_workspace(temp.path(), &request);
    let (workspace, editor, cx) = open_tab(cx, request.clone(), file.clone());

    // The keyboard is elsewhere in the window — another tab, a dock — when the file
    // changes behind the app's back.
    let workspace_focus =
        cx.read(|cx| workspace.read_with(cx, |workspace, cx| workspace.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&workspace_focus, cx));
    cx.run_until_parked();

    let mut changed = request.clone();
    changed.url = String::from("https://example.com/pets?team=other");
    rewrite_file(temp.path(), &file, &changed);

    let reloaded = wait_for_tab(&editor, cx, |editor, _| editor.draft().url == changed.url).await;
    assert!(
        reloaded,
        "a clean tab follows the file within the 1 s budget"
    );
    assert!(
        !cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx))),
        "the silent reload leaves the tab clean"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.disk_change().is_none())),
        "a clean tab never shows the changed-on-disk bar"
    );
    assert!(
        cx.update(|window, _cx| workspace_focus.is_focused(window)),
        "the silent reload leaves the keyboard where it was"
    );
    assert!(
        !url_field_focused(&editor, cx),
        "a background tab's reload never puts typing in the background"
    );
}

#[gpui::test]
async fn reload_click_puts_the_cursor_back_in_the_url_field(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let temp = TempDir::new().expect("a scratch workspace folder");
    let request = request();
    let file = fixture_workspace(temp.path(), &request);
    let (_workspace, editor, cx) = open_tab(cx, request.clone(), file.clone());

    edit_url(cx);
    let mut changed = request.clone();
    changed.url = String::from("https://example.com/pets?team=other");
    rewrite_file(temp.path(), &file, &changed);

    assert!(
        wait_for_tab(&editor, cx, |editor, _| editor.disk_change().is_some()).await,
        "the dirty tab shows the changed-on-disk bar"
    );
    draw_window(cx);
    click_control(RELOAD_SELECTOR, cx);

    assert!(
        url_field_focused(&editor, cx),
        "the user's Reload click puts typing back in the URL field"
    );
}

#[gpui::test]
async fn keep_mine_stays_quiet_until_the_file_changes_again(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let temp = TempDir::new().expect("a scratch workspace folder");
    let request = request();
    let file = fixture_workspace(temp.path(), &request);
    let (_workspace, editor, cx) = open_tab(cx, request.clone(), file.clone());

    edit_url(cx);
    let mut changed = request.clone();
    changed.url = String::from("https://example.com/pets?team=other");
    rewrite_file(temp.path(), &file, &changed);

    assert!(
        wait_for_tab(&editor, cx, |editor, _| editor.disk_change().is_some()).await,
        "the dirty tab shows the changed-on-disk bar"
    );
    draw_window(cx);
    click_control(KEEP_MINE_SELECTOR, cx);

    // Later batches of the very version the user dismissed — the every-`Rescan` echo of
    // an unchanged file — must not bring the bar back.
    rewrite_file(temp.path(), &file, &changed);
    settle(cx).await;
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.disk_change().is_none())),
        "Keep mine stays quiet while the file still holds the version it dismissed"
    );

    // A version the user has NOT dismissed shows the bar again.
    let mut third = request.clone();
    third.url = String::from("https://example.com/pets?team=third");
    rewrite_file(temp.path(), &file, &third);
    assert!(
        wait_for_tab(&editor, cx, |editor, _| editor.disk_change().is_some()).await,
        "a version the user never dismissed shows the bar again"
    );
}

#[gpui::test]
async fn a_file_that_matches_the_draft_clears_the_dirty_dot_without_a_bar(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let temp = TempDir::new().expect("a scratch workspace folder");
    let request = request();
    let file = fixture_workspace(temp.path(), &request);
    let (_workspace, editor, cx) = open_tab(cx, request.clone(), file.clone());

    edit_url(cx);
    let edited = cx.read(|cx| editor.read_with(cx, |editor, _| editor.draft().clone()));

    // The file now holds exactly this tab's draft — a teammate made the same edit.
    rewrite_file(temp.path(), &file, &edited);

    assert!(
        wait_for_tab(&editor, cx, |editor, cx| !editor.is_dirty(cx)).await,
        "a file that already holds the draft leaves the tab clean"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.disk_change().is_none())),
        "a file that already holds the draft needs no changed-on-disk bar"
    );
}

#[gpui::test]
async fn a_save_echo_shows_no_bar_and_clears_the_dirty_dot(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let temp = TempDir::new().expect("a scratch workspace folder");
    let request = request();
    let file = fixture_workspace(temp.path(), &request);
    let (_workspace, editor, cx) = open_tab(cx, request.clone(), file.clone());

    edit_url(cx);
    cx.simulate_keystrokes(save_keys(cx));
    cx.run_until_parked();

    // The tab's own write comes back through the watcher (S42a's carry-forward): it is
    // not a change to tell the user about.
    settle(cx).await;
    assert!(
        !cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx))),
        "the save cleared the dirty dot"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.disk_change().is_none())),
        "the echo of the tab's own save never shows the changed-on-disk bar"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, _| editor.file_gone().is_none())),
        "and never flags the file as gone"
    );
}
