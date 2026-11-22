//! The add/edit form's own keys (#180): Enter in a field saves the cookie and Escape
//! cancels the form — without closing the dialog (the `Decision:` lines in
//! [`crate::cookie_form`]). The closing keys are pressed for real, so this covers the
//! wiring itself: the field's `Submitted` event reaching `cookie_form`'s
//! `on_field_event`, and the form's `on_key_down` seeing the Escape its focused field
//! bubbles up. `cookie_form_tests.rs` drives the form through its public API and is
//! frozen; these are the keystrokes. Fixture values are obviously fake
//! (`.agents/rules/tests.md`).

use std::sync::{Arc, Mutex};

use gpui::{Entity, TestAppContext, VisualTestContext};
use http_engine::{CookieJar, load_jar};
use pustman_actions::cookies::Manage;
use storage::LocalStore;
use ui::test_support::{draw_window, init_test, press_key};
use workspace::Workspace;

use super::{CookieField, CookieForm};
use crate::cookie_dialog::{CookieDialog, CookieDialogInputs, install_inputs};

/// The workspace the fixture's cookies belong to.
const WORKSPACE_ID: &str = "workspace-a";

/// Obviously fake cookie values (`.agents/rules/tests.md`).
const LOCALE_VALUE: &str = "token-REDACTED-locale";

/// The local store the jar persists through: in memory and migrated (the shape
/// `cookie_persistence`'s own tests open their store with).
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// A workspace window with this crate wired up, drawn and active.
fn dialog_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        // Trap: without `text_input::init` the fields' editing keys are unbound and
        // Enter never turns into the field's `Submitted` event (the `find` key tests'
        // trap).
        text_input::init(cx);
        crate::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (workspace, cx)
}

/// Install `store` and `jar` as the dialog's inputs.
fn install(store: Arc<Mutex<LocalStore>>, jar: Arc<Mutex<CookieJar>>, cx: &mut VisualTestContext) {
    cx.update(|_window, cx| {
        install_inputs(
            cx,
            CookieDialogInputs {
                workspace_id: WORKSPACE_ID.to_string(),
                store,
                jar,
            },
        )
    });
}

/// The dialog the workspace's modal layer is showing, if one is open.
fn cookie_dialog_of(
    workspace: &Entity<Workspace>,
    cx: &VisualTestContext,
) -> Option<Entity<CookieDialog>> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace.modal_layer().active_modal::<CookieDialog>()
        })
    })
}

/// Open the dialog the `cookies::Manage` action's way.
fn open_from_action(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Entity<CookieDialog> {
    cx.dispatch_action(Manage);
    cx.run_until_parked();
    draw_window(cx);
    cookie_dialog_of(workspace, cx).expect("dispatching cookies::Manage opens the dialog")
}

/// Open the add form the keyboard's way: the "Add cookie" control under focus, one
/// Enter. The form's first field has the keyboard after it opens (the dialog's
/// `mount_form`), which is where every keystroke below is aimed.
fn open_add_form(dialog: &Entity<CookieDialog>, cx: &mut VisualTestContext) -> Entity<CookieForm> {
    let focus_handle = cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.add_focus_handle()));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);
    cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.form()))
        .expect("the Add cookie control opens the form")
}

/// Fill one field through the form's public API — what typing lands as.
fn fill(form: &Entity<CookieForm>, cx: &mut VisualTestContext, field: CookieField, text: &str) {
    cx.update(|_window, cx| form.update(cx, |form, cx| form.set_field(field, text, cx)));
}

/// The dialog's inputs and the shell every key test starts in.
struct Fixture {
    store: Arc<Mutex<LocalStore>>,
    jar: Arc<Mutex<CookieJar>>,
    workspace: Entity<Workspace>,
    dialog: Entity<CookieDialog>,
}

/// The dialog with its add form open, one cookie fully typed in and the Name field
/// under the keyboard — the state both key tests start from. Only the closing key is a
/// keystroke.
fn form_ready_to_save(cx: &mut TestAppContext) -> (Fixture, &mut VisualTestContext) {
    let store = open_store();
    let jar = Arc::new(Mutex::new(CookieJar::new()));
    let (workspace, cx) = dialog_window(cx);
    install(Arc::clone(&store), Arc::clone(&jar), cx);
    let dialog = open_from_action(&workspace, cx);
    let form = open_add_form(&dialog, cx);
    fill(&form, cx, CookieField::Name, "locale");
    fill(&form, cx, CookieField::Value, LOCALE_VALUE);
    fill(&form, cx, CookieField::Domain, "api.example.test");
    fill(&form, cx, CookieField::Path, "/");
    (
        Fixture {
            store,
            jar,
            workspace,
            dialog,
        },
        cx,
    )
}

#[gpui::test]
async fn enter_in_a_field_saves_the_cookie(cx: &mut TestAppContext) {
    let background = cx.executor();
    let (fixture, cx) = form_ready_to_save(cx);
    let dialog = &fixture.dialog;

    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.form().is_none())),
        "Enter saves and takes the form down"
    );
    let rows = cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.displayed_rows()));
    let saved = rows
        .iter()
        .find(|row| row.name == "locale")
        .expect("the saved cookie gets a row");
    assert_eq!(
        saved.value, LOCALE_VALUE,
        "the row shows the value that was typed"
    );

    // The save reached the store, not just the screen.
    let reloaded = load_jar(&background, fixture.store, WORKSPACE_ID.to_string())
        .await
        .expect("the jar reloads from the store");
    let entry = reloaded
        .list()
        .into_iter()
        .find(|entry| entry.name == "locale")
        .expect("the saved cookie is stored");
    assert_eq!(
        entry.value, LOCALE_VALUE,
        "Enter reached the jar, not just the screen"
    );
}

#[gpui::test]
async fn escape_cancels_the_form_without_closing_the_dialog(cx: &mut TestAppContext) {
    let (fixture, cx) = form_ready_to_save(cx);
    let dialog = &fixture.dialog;

    press_key(cx, "escape");
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.form().is_none())),
        "Escape cancels the form"
    );
    assert!(
        cookie_dialog_of(&fixture.workspace, cx).is_some(),
        "Escape stops at the form: the dialog it lives in stays open"
    );
    let rows = cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.displayed_rows()));
    assert!(
        !rows.iter().any(|row| row.name == "locale"),
        "a cancelled form adds no row"
    );
    assert!(
        fixture
            .jar
            .lock()
            .expect("the jar is not poisoned")
            .list()
            .is_empty(),
        "a cancelled form reaches neither the jar nor the store"
    );
}
