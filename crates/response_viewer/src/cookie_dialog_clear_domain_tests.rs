//! Clearing one domain from the Manage Cookies dialog (REQ-HTTP-08): the domain
//! header's "Clear" button removes every cookie under that domain — in the listed rows
//! and in the jar `load_jar` finds afterwards — and leaves every other domain's cookies
//! alone. Driven the way the delete test drives its button: the control's own focus
//! handle under the keyboard and one Enter. Fixture values are obviously fake
//! (`.agents/rules/tests.md`).

use std::sync::{Arc, Mutex};

use gpui::{Entity, TestAppContext, VisualTestContext};
use http_engine::{CookieJar, load_jar};
use pustman_actions::cookies::Manage;
use storage::LocalStore;
use ui::test_support::{draw_window, init_test, press_key};
use url::Url;
use workspace::Workspace;

use super::{CookieDialog, CookieDialogInputs, install_inputs};

/// The workspace every fixture's cookies belong to.
const WORKSPACE_ID: &str = "workspace-a";

/// Obviously fake cookie values (`.agents/rules/tests.md`).
const SID_VALUE: &str = "token-REDACTED-example";
const THEME_VALUE: &str = "token-REDACTED-theme";
const ASSET_VALUE: &str = "token-REDACTED-asset";

/// A jar of three fixture cookies: two under `api.example.test` (paths `/` and `/api`) —
/// the domain the test clears — and one under `cdn.example.test`, which must survive.
/// Built through the jar's own `Set-Cookie` path, so what the dialog lists is what a
/// response would have left there.
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

/// Open the dialog the `cookies::Manage` action's way over `store`'s and `jar`'s inputs.
fn open_dialog(
    workspace: &Entity<Workspace>,
    store: Arc<Mutex<LocalStore>>,
    jar: Arc<Mutex<CookieJar>>,
    cx: &mut VisualTestContext,
) -> Entity<CookieDialog> {
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
    cx.dispatch_action(Manage);
    cx.run_until_parked();
    draw_window(cx);
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace.modal_layer().active_modal::<CookieDialog>()
        })
    })
    .expect("dispatching cookies::Manage opens the dialog")
}

/// Turn the domain header's "Clear" button the keyboard's way: focus the button's own
/// handle and press Enter.
fn clear_domain(dialog: &Entity<CookieDialog>, cx: &mut VisualTestContext, domain: &str) {
    let focus_handle = cx
        .read(|cx| dialog.read_with(cx, |dialog, _| dialog.clear_focus_handle(domain)))
        .expect("the domain header has a Clear button");
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
}

#[gpui::test]
async fn clear_domain_removes_every_cookie_of_that_domain(cx: &mut TestAppContext) {
    let background = cx.executor();
    let store = open_store();
    let jar = Arc::new(Mutex::new(fixture_jar()));
    let (workspace, cx) = dialog_window(cx);
    let dialog = open_dialog(&workspace, Arc::clone(&store), Arc::clone(&jar), cx);

    clear_domain(&dialog, cx, "api.example.test");

    let rows = cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.displayed_rows()));
    assert!(
        !rows.iter().any(|row| row.domain == "api.example.test"),
        "the cleared domain's header and cookies are gone"
    );
    assert!(
        rows.iter().any(|row| row.name == "asset"),
        "the other domain's cookie keeps its row"
    );

    // The clear reached the store, not just the screen: reload the jar from the store
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
        !names.iter().any(|name| name == "sid" || name == "theme"),
        "the cleared domain's cookies are no longer stored"
    );
    assert!(
        names.iter().any(|name| name == "asset"),
        "the other domain's cookie is still stored"
    );
}
