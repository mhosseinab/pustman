//! The Manage Cookies dialog is driven the way a user drives it: the Cookies tab's
//! "Manage cookies" button and the `cookies::Manage` action, a real `CookieJar` and the
//! real `storage::LocalStore` behind `http_engine::cookie_persistence`. Every assertion
//! is on the rows the dialog displays (via [`super::CookieDialog::displayed_rows`],
//! exactly what `render` draws) and — for the delete — on what `load_jar` finds after
//! it: a dialog that lists a stubbed row model, or a delete that only hides the row,
//! fails them. Fixture values are obviously fake (`.agents/rules/tests.md`).

use std::sync::{Arc, Mutex};

use gpui::{AnyView, AppContext, Entity, TestAppContext, VisualTestContext};
use http_engine::{CookieJar, load_jar};
use pustman_actions::cookies::Manage;
use storage::LocalStore;
use ui::test_support::{draw_window, init_test, press_key};
use url::Url;
use workspace::Workspace;

use super::{CookieDialog, CookieDialogInputs, install_inputs};
use crate::cookies_tab::CookiesTab;

/// The workspace every fixture's cookies belong to.
const WORKSPACE_ID: &str = "workspace-a";

/// Obviously fake cookie values (`.agents/rules/tests.md`).
const SID_VALUE: &str = "token-REDACTED-example";
const THEME_VALUE: &str = "token-REDACTED-theme";
const ASSET_VALUE: &str = "token-REDACTED-asset";

/// A jar of three fixture cookies: two under `api.example.test` (paths `/` and `/api`)
/// and one under `cdn.example.test`. Built through the jar's own `Set-Cookie` path, so
/// what the dialog lists is what a response would have left there.
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
    jar.store_response_cookies(
        &url("https://cdn.example.test/"),
        &[format!("asset={ASSET_VALUE}; Path=/")],
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

/// Open the dialog the tab's way: the Cookies tab in the workspace's center pane, its
/// "Manage cookies" button under the keyboard, and one Enter — which GPUI turns into
/// exactly one click on the focused control.
fn open_from_cookies_tab(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> Entity<CookieDialog> {
    let cookies_tab = cx.new(CookiesTab::new);
    cx.update(|_window, cx| {
        workspace.update(cx, |workspace, cx| {
            workspace.center_pane().update(cx, |pane, cx| {
                pane.add_item(AnyView::from(cookies_tab.clone()), cx);
            });
        });
    });
    draw_window(cx);
    let manage_focus = cx.read(|cx| cookies_tab.read_with(cx, |tab, _| tab.manage_focus_handle()));
    cx.update(|window, cx| window.focus(&manage_focus, cx));
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);
    cookie_dialog_of(workspace, cx).expect("the Manage cookies button opens the dialog")
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

/// Turn the (domain, path, name) row's delete button the keyboard's way: focus the
/// button's own handle and press Enter.
fn delete_cookie(
    dialog: &Entity<CookieDialog>,
    cx: &mut VisualTestContext,
    domain: &str,
    path: &str,
    name: &str,
) {
    let focus_handle = cx
        .read(|cx| {
            dialog.read_with(cx, |dialog, _| {
                dialog.delete_focus_handle(domain, path, name)
            })
        })
        .expect("the cookie row has a delete button");
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
}

#[gpui::test]
fn dialog_lists_cookies_by_domain(cx: &mut TestAppContext) {
    let store = open_store();
    let jar = Arc::new(Mutex::new(fixture_jar()));
    let (workspace, cx) = dialog_window(cx);
    install(store, Arc::clone(&jar), cx);
    let dialog = open_from_cookies_tab(&workspace, cx);

    let rows = cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.displayed_rows()));
    let shown: Vec<(bool, &str, &str)> = rows
        .iter()
        .map(|row| (row.group_header, row.domain.as_str(), row.name.as_str()))
        .collect();
    assert_eq!(
        shown,
        vec![
            (true, "api.example.test", "2 cookies"),
            (false, "api.example.test", "theme"),
            (false, "api.example.test", "sid"),
            (true, "cdn.example.test", "1 cookie"),
            (false, "cdn.example.test", "asset"),
        ],
        "the jar is listed grouped by domain, in (domain, path, name) order"
    );
    let sid = rows
        .iter()
        .find(|row| row.name == "sid")
        .expect("the sid cookie gets a row");
    assert_eq!(sid.value, SID_VALUE, "the row shows the cookie's value");
    assert_eq!(sid.path, "/api", "the row shows the cookie's path");
    assert_eq!(sid.expires, "Session", "a session cookie says so");
}

#[gpui::test]
async fn delete_cookie_removes_row(cx: &mut TestAppContext) {
    let background = cx.executor();
    let store = open_store();
    let jar = Arc::new(Mutex::new(fixture_jar()));
    let (workspace, cx) = dialog_window(cx);
    install(Arc::clone(&store), Arc::clone(&jar), cx);
    let dialog = open_from_action(&workspace, cx);

    delete_cookie(&dialog, cx, "api.example.test", "/api", "sid");

    let rows = cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.displayed_rows()));
    assert!(
        !rows.iter().any(|row| row.name == "sid"),
        "the deleted cookie's row is gone"
    );
    assert!(
        rows.iter().any(|row| row.name == "theme"),
        "the surviving cookie keeps its row"
    );

    // The delete reached the store, not just the screen: reload the jar from the store
    // the way the dialog loads it.
    let reloaded = load_jar(&background, store, WORKSPACE_ID.to_string())
        .await
        .expect("the jar reloads from the store");
    let names: Vec<String> = reloaded
        .list()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert!(
        !names.iter().any(|name| name == "sid"),
        "the deleted cookie is no longer stored"
    );
    assert!(
        names.iter().any(|name| name == "theme"),
        "the surviving cookie is still stored"
    );
}
