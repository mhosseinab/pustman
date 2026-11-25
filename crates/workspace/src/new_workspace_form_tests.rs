//! The New Workspace form driven the way a user drives it: `CreateWorkspace` opens the
//! form (never a folder dialog), the fields are filled, Create or Enter is pressed — and
//! the outcome is what the user sees: the workspace folder on disk, the window's open
//! workspace, the form gone, or the inline error that keeps Create disabled.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use pustman_actions::workspace::CreateWorkspace;
use storage::LocalStore;
use ui::prelude::FormField;
use ui::test_support::{draw_window, init_test, press_key};

use super::{
    CREATE_BUTTON_SELECTOR, LOCATION_FIELD, LOCATION_MISSING, NAME_FIELD, NewWorkspaceForm,
    OPEN_INSTEAD_BUTTON_SELECTOR,
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
