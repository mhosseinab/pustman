//! The New Workspace form driven the way a user drives it: `CreateWorkspace` opens the
//! form (never a folder dialog), the fields are filled, Create or Enter is pressed — and
//! the outcome is what the user sees: the workspace folder on disk, the window's open
//! workspace, the form gone, or the inline error that keeps Create disabled.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gpui::{Entity, Focusable, Modifiers, SharedString, TestAppContext, VisualTestContext};
use pustman_actions::workspace::CreateWorkspace;
use storage::LocalStore;
use ui::prelude::FormField;
use ui::test_support::{draw_window, init_test, press_key};

use super::{
    CREATE_BUTTON_SELECTOR, FAILURE_SELECTOR, LOCATION_FIELD, LOCATION_MISSING, NAME_FIELD,
    NewWorkspaceForm, OPEN_INSTEAD_BUTTON_SELECTOR, TARGET_ERROR_SELECTOR,
};
use crate::workspace_open::WorkspaceOpen;
use crate::{Workspace, init};

fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// A workspace window on the app's init order, with the store and the text fields' keys.
fn workspace_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    let store = open_store();
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        init(cx);
        WorkspaceOpen::install_local_store(store, cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    (workspace, cx)
}

/// `CreateWorkspace` → the form on screen.
fn open_form(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Entity<NewWorkspaceForm> {
    cx.dispatch_action(CreateWorkspace);
    cx.run_until_parked();
    assert!(
        !cx.did_prompt_for_paths(),
        "CreateWorkspace opens the form, not a folder dialog"
    );
    cx.read(|cx| {
        workspace
            .read(cx)
            .modal_layer()
            .active_modal::<NewWorkspaceForm>()
    })
    .expect("CreateWorkspace opens the New Workspace form")
}

/// Replace a field's text the way typing would, then let the location check land.
fn fill(
    form: &Entity<NewWorkspaceForm>,
    name: Option<&str>,
    location: Option<&Path>,
    cx: &mut VisualTestContext,
) {
    cx.update(|_window, cx| {
        let (name_input, location_input) = {
            let form = form.read(cx);
            (form.name.clone(), form.location.clone())
        };
        if let Some(name) = name {
            name_input.update(cx, |input, cx| input.set_text(name, cx));
        }
        if let Some(location) = location {
            let text = location.to_string_lossy().into_owned();
            location_input.update(cx, |input, cx| input.set_text(&text, cx));
        }
    });
    cx.run_until_parked();
    draw_window(cx);
}

fn leak(selector: String) -> &'static str {
    String::leak(selector)
}

fn can_create(form: &Entity<NewWorkspaceForm>, cx: &mut VisualTestContext) -> bool {
    cx.read(|cx| form.read(cx).can_create(cx))
}

fn open_root(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    cx.read(|cx| {
        WorkspaceOpen::for_window(workspace.entity_id(), cx)
            .and_then(|opener| opener.read(cx).root().map(Path::to_path_buf))
    })
}

fn form_is_open(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> bool {
    cx.read(|cx| {
        workspace
            .read(cx)
            .modal_layer()
            .active_modal::<NewWorkspaceForm>()
            .is_some()
    })
}

#[gpui::test]
fn new_workspace_form_creates_and_opens_the_workspace(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let (workspace, cx) = workspace_window(cx);
    let form = open_form(&workspace, cx);

    fill(&form, Some("Commerce API"), Some(directory.path()), cx);
    assert!(
        can_create(&form, cx),
        "a named, writable location enables Create"
    );

    let create = cx
        .debug_bounds(CREATE_BUTTON_SELECTOR)
        .expect("the Create button is on screen");
    cx.simulate_click(create.center(), Modifiers::none());
    cx.run_until_parked();

    let target = directory.path().join("commerce-api");
    assert!(
        target.join("pustman.json").is_file(),
        "Create writes the workspace at <location>/<slug of name>"
    );
    assert_eq!(
        open_root(&workspace, cx).as_deref(),
        Some(target.as_path()),
        "the new workspace is open in this window"
    );
    assert!(
        !form_is_open(&workspace, cx),
        "the form closes once the workspace is open"
    );
}

#[gpui::test]
fn new_workspace_form_submits_on_enter_from_the_name_field(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let (workspace, cx) = workspace_window(cx);
    let form = open_form(&workspace, cx);
    fill(&form, Some("Orders"), Some(directory.path()), cx);

    // Name has focus from the moment the form opens.
    press_key(cx, "enter");
    cx.run_until_parked();

    assert!(
        directory
            .path()
            .join("orders")
            .join("pustman.json")
            .is_file(),
        "Enter in the form creates the workspace"
    );
    assert!(!form_is_open(&workspace, cx));
}

#[gpui::test]
fn new_workspace_form_validates_inline(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let existing = directory.path().join("billing");
    std::fs::create_dir(&existing).expect("the existing workspace folder is created");
    std::fs::write(existing.join("pustman.json"), b"{}").expect("the workspace file is written");
    let (workspace, cx) = workspace_window(cx);
    let form = open_form(&workspace, cx);

    // An empty name, once the user has been in the field.
    fill(&form, Some("x"), Some(directory.path()), cx);
    fill(&form, Some(""), None, cx);
    assert!(
        cx.debug_bounds(leak(FormField::error_selector(NAME_FIELD)))
            .is_some(),
        "an empty name shows its error under the Name field"
    );
    assert!(
        !can_create(&form, cx),
        "Create stays disabled without a name"
    );

    // A location that does not exist.
    let missing = directory.path().join("missing");
    fill(&form, Some("Payments"), Some(&missing), cx);
    assert!(
        cx.debug_bounds(leak(FormField::error_selector(LOCATION_FIELD)))
            .is_some(),
        "a missing folder shows its error under the Location field: {LOCATION_MISSING}"
    );
    assert!(
        cx.debug_bounds(leak(FormField::error_selector(NAME_FIELD)))
            .is_none(),
        "a valid name shows no error"
    );
    assert!(
        !can_create(&form, cx),
        "Create stays disabled for a missing folder"
    );

    // A target that is already a workspace.
    fill(&form, Some("Billing"), Some(directory.path()), cx);
    assert!(
        cx.debug_bounds(OPEN_INSTEAD_BUTTON_SELECTOR).is_some(),
        "an existing workspace at the target is offered to open instead"
    );
    assert!(
        !can_create(&form, cx),
        "Create never overwrites a workspace"
    );

    // Pressing Enter on an invalid form writes nothing.
    fill(&form, Some("Payments"), Some(&missing), cx);
    press_key(cx, "enter");
    cx.run_until_parked();
    assert!(!missing.exists(), "nothing is written for an invalid form");
    let entries: Vec<_> = std::fs::read_dir(directory.path())
        .expect("the temp dir reads")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "only the fixture workspace is in the folder"
    );
    assert!(
        form_is_open(&workspace, cx),
        "the form stays open while it is invalid"
    );
}

#[gpui::test]
fn new_workspace_form_cancel_closes_without_writing(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let (workspace, cx) = workspace_window(cx);
    let form = open_form(&workspace, cx);
    fill(&form, Some("Unused"), Some(directory.path()), cx);

    press_key(cx, "escape");
    cx.run_until_parked();

    assert!(!form_is_open(&workspace, cx), "Escape closes the form");
    assert!(
        !directory.path().join("unused").exists(),
        "closing the form writes nothing"
    );
    assert_eq!(open_root(&workspace, cx), None, "no workspace was opened");
}

/// Whether `handle` has keyboard focus in the test window.
fn is_focused(handle: &gpui::FocusHandle, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, _cx| handle.is_focused(window))
}

#[gpui::test]
fn new_workspace_form_refuses_a_folder_that_exists_and_is_not_empty(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let taken = directory.path().join("notes");
    std::fs::create_dir(&taken).expect("the occupied folder is created");
    std::fs::write(taken.join("todo.txt"), b"mine").expect("its file is written");
    std::fs::create_dir(directory.path().join("empty")).expect("the empty folder is created");
    let (workspace, cx) = workspace_window(cx);
    let form = open_form(&workspace, cx);

    fill(&form, Some("Notes"), Some(directory.path()), cx);
    assert!(
        cx.debug_bounds(TARGET_ERROR_SELECTOR).is_some(),
        "a non-empty folder at the target is refused inline"
    );
    assert!(
        !can_create(&form, cx),
        "Create never adopts someone's folder"
    );
    press_key(cx, "enter");
    cx.run_until_parked();
    assert!(
        !taken.join("pustman.json").exists(),
        "nothing is written into the occupied folder"
    );

    fill(&form, Some("Empty"), None, cx);
    assert!(
        cx.debug_bounds(TARGET_ERROR_SELECTOR).is_none(),
        "an existing empty folder is fine"
    );
    press_key(cx, "enter");
    cx.run_until_parked();
    assert!(
        directory.path().join("empty/pustman.json").is_file(),
        "the workspace is created in the empty folder"
    );
}

#[gpui::test]
fn create_named_refuses_a_folder_that_filled_up_after_validation(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let target = directory.path().join("late");
    std::fs::create_dir(&target).expect("the folder is created");
    // It filled up between the form's check and Create.
    std::fs::write(target.join("arrived.txt"), b"late").expect("a file arrives");
    let (workspace, cx) = workspace_window(cx);
    let finished: Rc<RefCell<Option<Result<(), SharedString>>>> = Rc::default();

    cx.update(|_window, cx| {
        let opener = WorkspaceOpen::for_window(workspace.entity_id(), cx)
            .expect("the window has open-workspace state");
        let finished = finished.clone();
        opener.update(cx, |opener, cx| {
            opener.create_named(
                target.clone(),
                String::from("Late"),
                move |result, _cx| {
                    *finished.borrow_mut() = Some(result);
                    true
                },
                cx,
            );
        });
    });
    cx.run_until_parked();

    let result = finished.borrow_mut().take().expect("the create finished");
    let reason = result.expect_err("a non-empty folder is refused inside the create");
    assert!(
        reason.contains("isn't empty"),
        "the reason names the problem: {reason}"
    );
    assert!(!target.join("pustman.json").exists(), "nothing is written");
    assert_eq!(open_root(&workspace, cx), None, "no workspace was opened");
}

#[gpui::test]
fn new_workspace_form_rejects_a_relative_location(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let form = open_form(&workspace, cx);

    fill(&form, Some("Orders"), Some(Path::new("work/apis")), cx);

    assert!(
        cx.debug_bounds(leak(FormField::error_selector(LOCATION_FIELD)))
            .is_some(),
        "a relative location shows its error under the Location field"
    );
    assert!(
        !can_create(&form, cx),
        "Create stays disabled for a relative path"
    );
}

#[gpui::test]
fn new_workspace_form_checks_the_folders_again_when_create_is_pressed(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let later = directory.path().join("later");
    let (workspace, cx) = workspace_window(cx);
    let form = open_form(&workspace, cx);
    fill(&form, Some("Orders"), Some(&later), cx);
    assert!(!can_create(&form, cx), "the folder is missing at first");

    // The user makes the folder in a file manager, then presses Enter.
    std::fs::create_dir(&later).expect("the folder is created meanwhile");
    press_key(cx, "enter");
    cx.run_until_parked();

    assert!(
        later.join("orders/pustman.json").is_file(),
        "Create re-checks the location, so the folder made meanwhile is accepted"
    );
    assert!(!form_is_open(&workspace, cx));
}

#[gpui::test]
fn new_workspace_form_keeps_tab_inside_the_dialog(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let form = open_form(&workspace, cx);
    draw_window(cx);
    let (name, location, cancel) = cx.read(|cx| {
        let form = form.read(cx);
        (
            form.name.read(cx).focus_handle(cx),
            form.location.read(cx).focus_handle(cx),
            form.cancel_focus.clone(),
        )
    });
    assert!(is_focused(&name, cx), "Name has focus when the form opens");

    press_key(cx, "tab");
    assert!(is_focused(&location, cx), "Tab moves from Name to Location");

    press_key(cx, "shift-tab");
    press_key(cx, "shift-tab");
    assert!(
        is_focused(&cancel, cx),
        "Shift-Tab from Name wraps to Cancel, the last control, never the workbench"
    );
    press_key(cx, "tab");
    assert!(is_focused(&name, cx), "Tab from Cancel wraps back to Name");
}

#[gpui::test]
fn closing_the_form_returns_focus_to_where_it_was(cx: &mut TestAppContext) {
    let (workspace, cx) = workspace_window(cx);
    let before = cx.read(|cx| workspace.read(cx).focus_handle(cx));
    cx.update(|window, cx| window.focus(&before, cx));
    draw_window(cx);
    let _form = open_form(&workspace, cx);
    assert!(!is_focused(&before, cx), "the form takes focus");

    press_key(cx, "escape");
    cx.run_until_parked();

    assert!(
        is_focused(&before, cx),
        "focus goes back where it was on close"
    );
}

#[gpui::test]
fn new_workspace_form_keeps_a_create_failure_inline(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let (workspace, cx) = workspace_window(cx);
    let form = open_form(&workspace, cx);
    // A folder name longer than the filesystem allows passes every check and fails the
    // create itself.
    let name = "a".repeat(300);
    fill(&form, Some(&name), Some(directory.path()), cx);
    assert!(can_create(&form, cx));

    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        form_is_open(&workspace, cx),
        "a failed create keeps the form open"
    );
    assert!(
        cx.debug_bounds(FAILURE_SELECTOR).is_some(),
        "the reason shows inline in the form"
    );
    assert_eq!(open_root(&workspace, cx), None, "no workspace was opened");
}

#[gpui::test]
fn open_it_instead_opens_the_existing_workspace(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let existing = directory.path().join("billing");
    std::fs::create_dir(&existing).expect("the workspace folder is created");
    std::fs::write(
        existing.join("pustman.json"),
        serde_json::to_vec(&serde_json::json!({
            "id": api_model::ItemId::new().to_string(),
            "name": "Billing",
            "format_version": workspace_store::FORMAT_VERSION,
        }))
        .expect("the fixture serializes"),
    )
    .expect("the workspace file is written");
    let (workspace, cx) = workspace_window(cx);
    let form = open_form(&workspace, cx);
    fill(&form, Some("Billing"), Some(directory.path()), cx);

    let open_instead = cx
        .debug_bounds(OPEN_INSTEAD_BUTTON_SELECTOR)
        .expect("Open it instead is offered");
    cx.simulate_click(open_instead.center(), Modifiers::none());
    cx.run_until_parked();

    assert_eq!(
        open_root(&workspace, cx).as_deref(),
        Some(existing.as_path()),
        "the existing workspace is open in this window"
    );
    assert!(!form_is_open(&workspace, cx), "the form closes");
}

#[gpui::test]
fn a_name_without_letters_or_digits_creates_a_workspace_folder(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("the temp dir must be created");
    let (workspace, cx) = workspace_window(cx);
    let form = open_form(&workspace, cx);
    fill(&form, Some("!!!"), Some(directory.path()), cx);

    assert_eq!(
        cx.read(|cx| form.read(cx).target(cx)),
        Some(directory.path().join("workspace")),
        "a name with no slug falls back to a folder called workspace"
    );
}
