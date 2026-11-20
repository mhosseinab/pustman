//! The add/edit form is driven the way a user drives it: the dialog's "Add cookie" and
//! per-row "Edit" controls under the keyboard, the form's own public input API, and a
//! real `CookieJar` with the real `storage::LocalStore` behind
//! `http_engine::cookie_persistence`. Every assertion is on the rows the dialog displays
//! and what `load_jar` finds afterwards — a form that only updates its fields, or a save
//! that only updates the screen, fails them. Fixture values are obviously fake
//! (`.agents/rules/tests.md`).

use std::sync::{Arc, Mutex};

use gpui::{Entity, FocusHandle, TestAppContext, VisualTestContext};
use http_engine::{CookieJar, load_jar};
use pustman_actions::cookies::Manage;
use storage::LocalStore;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc2822;
use ui::test_support::{draw_window, init_test, press_key};
use url::Url;
use workspace::Workspace;

use super::{CookieField, CookieForm};
use crate::cookie_dialog::{CookieDialog, CookieDialogInputs, DisplayedRow, install_inputs};

/// The workspace every fixture's cookies belong to.
const WORKSPACE_ID: &str = "workspace-a";

/// Obviously fake cookie values (`.agents/rules/tests.md`).
const SID_VALUE: &str = "token-REDACTED-example";
const THEME_VALUE: &str = "token-REDACTED-theme";
const EDITED_VALUE: &str = "token-REDACTED-rotated";
const ADDED_VALUE: &str = "token-REDACTED-added";

/// A far-future expiry typed into the form, in the form's RFC 2822 format — far enough
/// ahead that the jar never reads it as expired.
const EXPIRY_TEXT: &str = "Thu, 15 Jan 2099 12:00:00 +0000";

/// A jar of two fixture cookies under `api.example.test` (paths `/api` and `/`), built
/// through the jar's own `Set-Cookie` path, so what the form edits is what a response
/// would have left there.
fn fixture_jar() -> CookieJar {
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &url("https://api.example.test/api"),
        &[format!("sid={SID_VALUE}; Path=/api")],
    );
    jar.store_response_cookies(
        &url("https://api.example.test/"),
        &[format!("theme={THEME_VALUE}; Path=/")],
    );
    jar
}

fn url(text: &str) -> Url {
    Url::parse(text).expect("the fixture URL parses")
}

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
/// Enter — which GPUI turns into exactly one click on the focused control.
fn open_add_form(dialog: &Entity<CookieDialog>, cx: &mut VisualTestContext) -> Entity<CookieForm> {
    let focus_handle = cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.add_focus_handle()));
    open_form_via(&focus_handle, dialog, cx)
}

/// Open the edit form of the (domain, path, name) row the keyboard's way: the row's
/// Edit control under focus, one Enter.
fn open_edit_form(
    dialog: &Entity<CookieDialog>,
    cx: &mut VisualTestContext,
    domain: &str,
    path: &str,
    name: &str,
) -> Entity<CookieForm> {
    let focus_handle = cx
        .read(|cx| dialog.read_with(cx, |dialog, _| dialog.edit_focus_handle(domain, path, name)))
        .expect("the cookie row has an edit button");
    open_form_via(&focus_handle, dialog, cx)
}

/// Press Enter on `focus_handle` and return the form the dialog shows afterwards.
fn open_form_via(
    focus_handle: &FocusHandle,
    dialog: &Entity<CookieDialog>,
    cx: &mut VisualTestContext,
) -> Entity<CookieForm> {
    cx.update(|window, cx| window.focus(focus_handle, cx));
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);
    cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.form()))
        .expect("the control opens the form")
}

/// Fill one field through the form's public API — what typing lands as.
fn fill(form: &Entity<CookieForm>, cx: &mut VisualTestContext, field: CookieField, text: &str) {
    cx.update(|_window, cx| form.update(cx, |form, cx| form.set_field(field, text, cx)));
}

/// Save through the form's public API — what the Save button and Enter run.
fn submit(form: &Entity<CookieForm>, cx: &mut VisualTestContext) {
    cx.update(|_window, cx| form.update(cx, |form, cx| form.submit(cx)));
    cx.run_until_parked();
}

/// The dialog's rows as the user sees them.
fn displayed_rows(dialog: &Entity<CookieDialog>, cx: &VisualTestContext) -> Vec<DisplayedRow> {
    cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.displayed_rows()))
}

#[gpui::test]
async fn add_cookie_persists(cx: &mut TestAppContext) {
    let background = cx.executor();
    let store = open_store();
    let jar = Arc::new(Mutex::new(CookieJar::new()));
    let (workspace, cx) = dialog_window(cx);
    install(Arc::clone(&store), Arc::clone(&jar), cx);
    let dialog = open_from_action(&workspace, cx);
    let form = open_add_form(&dialog, cx);

    fill(&form, cx, CookieField::Name, "locale");
    fill(&form, cx, CookieField::Value, ADDED_VALUE);
    fill(&form, cx, CookieField::Domain, "api.example.test");
    fill(&form, cx, CookieField::Path, "/");
    fill(&form, cx, CookieField::Expires, EXPIRY_TEXT);
    cx.update(|_window, cx| form.update(cx, |form, cx| form.set_secure(true, cx)));
    submit(&form, cx);

    // The dialog lists the row — and the form is down again.
    let rows = displayed_rows(&dialog, cx);
    let added = rows
        .iter()
        .find(|row| row.name == "locale")
        .expect("the added cookie gets a row");
    assert_eq!(
        added.value, ADDED_VALUE,
        "the row shows the value that was typed"
    );
    assert!(!added.group_header, "the row is a cookie row");
    assert!(
        cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.form().is_none())),
        "a form whose save landed closes"
    );

    // The save reached the store, not just the screen: reload the jar from the store.
    let reloaded = load_jar(&background, store, WORKSPACE_ID.to_string())
        .await
        .expect("the jar reloads from the store");
    let entry = reloaded
        .list()
        .into_iter()
        .find(|entry| entry.name == "locale")
        .expect("the added cookie is stored");
    assert_eq!(entry.value, ADDED_VALUE, "the stored value is the new one");
    assert_eq!(entry.domain, "api.example.test");
    assert_eq!(entry.path, "/");
    assert!(entry.secure, "the Secure flag that was set is stored");
    let expected_expiry =
        OffsetDateTime::parse(EXPIRY_TEXT, &Rfc2822).expect("the fixture expiry parses");
    assert_eq!(
        entry.expires,
        Some(expected_expiry),
        "the expiry that was typed round-trips through the store"
    );
}

#[gpui::test]
async fn edit_cookie_updates_value(cx: &mut TestAppContext) {
    let background = cx.executor();
    let store = open_store();
    let jar = Arc::new(Mutex::new(fixture_jar()));
    let (workspace, cx) = dialog_window(cx);
    install(Arc::clone(&store), Arc::clone(&jar), cx);
    let dialog = open_from_action(&workspace, cx);
    let form = open_edit_form(&dialog, cx, "api.example.test", "/api", "sid");

    fill(&form, cx, CookieField::Value, EDITED_VALUE);
    submit(&form, cx);

    // The dialog row shows the new value and the old one is gone.
    let rows = displayed_rows(&dialog, cx);
    let sid = rows
        .iter()
        .find(|row| row.name == "sid")
        .expect("the edited cookie keeps its row");
    assert_eq!(sid.value, EDITED_VALUE, "the row shows the new value");
    assert!(
        !rows.iter().any(|row| row.value == SID_VALUE),
        "the old value is gone from the dialog"
    );

    // And so does the reloaded jar.
    let reloaded = load_jar(&background, Arc::clone(&store), WORKSPACE_ID.to_string())
        .await
        .expect("the jar reloads from the store");
    let entries = reloaded.list();
    let stored = entries
        .iter()
        .find(|entry| entry.name == "sid")
        .expect("the edited cookie is still stored");
    assert_eq!(
        stored.value, EDITED_VALUE,
        "the stored value is the new one"
    );
    assert!(
        !entries.iter().any(|entry| entry.value == SID_VALUE),
        "the old value is no longer stored"
    );

    // An invalid edit shows its error inline and persists nothing.
    let before = entries;
    let form = open_edit_form(&dialog, cx, "api.example.test", "/api", "sid");
    fill(&form, cx, CookieField::Name, "");
    submit(&form, cx);

    let error = cx
        .read(|cx| form.read_with(cx, |form, _| form.error()))
        .expect("the form shows an inline error");
    assert!(
        error.as_ref().contains("Name is required"),
        "the form names the missing field instead of failing silently"
    );
    assert!(
        cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.form().is_some())),
        "a refused save keeps the form open on the fields it failed on"
    );
    let after = load_jar(&background, store, WORKSPACE_ID.to_string())
        .await
        .expect("the jar reloads from the store")
        .list();
    assert_eq!(
        after, before,
        "an invalid edit reaches neither the jar nor the store"
    );
}
