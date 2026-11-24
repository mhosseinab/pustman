//! The save cycle's scope and ordering (S40c fix 1): `workspace::Save` writes one at a
//! time per tab, only for the request that has the keyboard, and only what the user asked
//! to save. Driven the way a user drives them — the real keystroke from the default
//! keymap — against a temp workspace on disk. The two-save tests let the writes race on
//! the randomized scheduler (`iterations`), the shape `http_engine`'s cookie-save order
//! tests take: the detached writes it replaces raced exactly there.
//!
//! Wired from `request_editor.rs`, not from `save.rs`, so the step's pinned `save::`
//! filter keeps selecting exactly the two `save::tests` tests.

use std::path::{Path, PathBuf};

use api_model::{HttpRequest, Request};
use gpui::{
    AppContext, Entity, Focusable, Keystroke, OperatingSystem, TestAppContext, VisualTestContext,
};
use ui::components::toast::toast_message_selector;
use ui::test_support::{draw_window, init_test};
use workspace::{Item, Workspace};
use workspace_store::ItemBody;

use crate::RequestEditor;
use crate::save::{RequestFile, install};

/// The request the tests edit and save: a GET named so its file has a stable name.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.name = String::from("Get pets");
    request.url = String::from("https://example.com/ping");
    request
}

/// Write `request` into the workspace at `root` the way the app's workspace wiring finds
/// it: on disk at the placement the store chose, registered in [`install`] with that
/// placement.
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

/// A workspace window holding `request` as its one tab, on the app's real init order and
/// the real default keymap, with the cursor where a user editing a request starts: the
/// URL field.
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

/// Dispatch one keystroke without the "run the background executor until it is parked"
/// tail that `simulate_keystrokes` and `simulate_input` have: a write queued by one press
/// must stay in flight while the user keeps editing and pressing, or the writes never
/// overlap and the order they race in cannot be observed.
fn press(cx: &mut VisualTestContext, keystroke: &str) {
    let os = cx.read(|cx| cx.platform_info().os);
    let keystroke = Keystroke::parse_with_os(keystroke, os).expect("the keystroke parses");
    cx.update(|window, cx| {
        window.dispatch_keystroke(keystroke, cx);
    });
}

/// Type `suffix` onto the URL — an edit of the draft the user can see.
fn extend_url(cx: &mut VisualTestContext, suffix: &str) {
    press(cx, "end");
    for character in suffix.chars() {
        press(cx, &character.to_string());
    }
}

/// Press the platform's `workspace::Save` keystroke.
fn press_save(cx: &mut VisualTestContext) {
    press(cx, save_keys(cx));
}

/// The draft's URL right now — what the user sees and what a save must write.
fn draft_url(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> String {
    cx.read(|cx| editor.read_with(cx, |editor, _| editor.draft().url.clone()))
}

/// The request the file at `written` holds.
fn file_request(root: &Path, written: &Path) -> HttpRequest {
    let on_disk = std::fs::read_to_string(root.join(written)).expect("the request's file is there");
    let request: Request = serde_json::from_str(&on_disk).expect("the file is the request's JSON");
    let Request::Http(request) = request else {
        panic!("the file holds an HTTP request");
    };
    request
}

#[gpui::test(iterations = 25)]
fn two_saves_with_an_edit_between_them_leave_the_second_draft_saved_and_the_tab_clean(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let request = request();
    let file = place_request(directory.path(), &request);
    let written = file
        .item
        .path
        .clone()
        .expect("the fixture request has a file");
    let (_workspace, editor, cx) = open_editor(cx, request, vec![file]);

    // The user saves, edits the URL again while that write is still going, and saves
    // once more: the second press is the one whose draft must end up on disk.
    extend_url(cx, "/v1");
    press_save(cx);
    extend_url(cx, "/v2");
    let second_draft = draft_url(&editor, cx);
    press_save(cx);
    cx.run_until_parked();

    let saved = file_request(directory.path(), &written);
    assert_eq!(
        saved.url, second_draft,
        "the file holds the second draft, not an older write landing later"
    );
    assert!(
        !cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx))),
        "the tab is clean: everything on screen is on disk"
    );
}

#[gpui::test(iterations = 25)]
fn an_edit_made_while_a_save_is_in_flight_keeps_the_tab_dirty(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let request = request();
    let file = place_request(directory.path(), &request);
    let written = file
        .item
        .path
        .clone()
        .expect("the fixture request has a file");
    let (_workspace, editor, cx) = open_editor(cx, request, vec![file]);

    extend_url(cx, "/v1");
    let saved_draft = draft_url(&editor, cx);
    press_save(cx);
    // An edit while the write runs — but no second press: the user never asked for this
    // one to be saved.
    extend_url(cx, "/v2");
    cx.run_until_parked();

    assert_eq!(
        file_request(directory.path(), &written).url,
        saved_draft,
        "the write lands the draft the user saved"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx))),
        "an edit the user did not ask to save keeps the tab dirty"
    );
}

#[gpui::test]
fn ctrl_s_leaves_the_file_unwritten_when_another_focusable_has_the_keyboard(
    cx: &mut TestAppContext,
) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let request = request();
    let file = place_request(directory.path(), &request);
    let written = file
        .item
        .path
        .clone()
        .expect("the fixture request has a file");
    let (workspace, editor, cx) = open_editor(cx, request, vec![file]);
    let before = std::fs::read(directory.path().join(&written)).expect("the fixture file is there");

    extend_url(cx, "/v2");
    // The keyboard is outside the request editor — a dock panel, a modal or a picker has
    // it in the app; the workspace shell's own focus handle is the second focusable.
    let shell = cx.read(|cx| workspace.read_with(cx, |workspace, cx| workspace.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&shell, cx));
    draw_window(cx);
    press_save(cx);
    cx.run_until_parked();

    let after = std::fs::read(directory.path().join(&written)).expect("the fixture file is there");
    assert_eq!(
        after, before,
        "Ctrl-S outside the editor writes nothing behind it"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx))),
        "the tab keeps its unsaved edit"
    );
}

#[gpui::test]
fn ctrl_s_on_a_request_with_no_file_says_so_in_a_toast(cx: &mut TestAppContext) {
    let request = request();
    // No `install`: this request is in no workspace yet — `NewRequest`'s fresh tab.
    let (_workspace, editor, cx) = open_editor(cx, request.clone(), Vec::new());

    extend_url(cx, "/v2");
    press_save(cx);
    cx.run_until_parked();
    draw_window(cx);

    let message = format!(
        "Could not save {} — it has no file in a workspace yet",
        request.name
    );
    let selector: &'static str = toast_message_selector(&message).leak();
    assert!(
        cx.debug_bounds(selector).is_some(),
        "a request with no file says so instead of failing quietly"
    );
    assert!(
        cx.read(|cx| editor.read_with(cx, |editor, cx| editor.is_dirty(cx))),
        "nothing was written, so the edit stays unsaved"
    );
}
