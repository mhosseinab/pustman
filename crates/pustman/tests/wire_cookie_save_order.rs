//! The cookie wire's saves land in the order the jar changed (S33p review 1, finding 1):
//! a later save — the jar after a server cleared a cookie — is never overwritten by an
//! earlier snapshot that still held it, so a cleared session cookie does not come back
//! after a restart. `iterations` shuffles the background executor's task order, which is
//! where two unordered saves race.

// This file is a test crate: `clippy.toml` allows `expect` in tests, but clippy only
// recognises `#[test]` fns as such, so the helpers below ask for the same courtesy.
#![allow(clippy::expect_used)]

use std::sync::{Arc, Mutex};

use console::ConsoleStore;
use gpui::{TestAppContext, VisualTestContext};
use http_engine::CookieJar;
use pustman::{AppCookieJar, StartupProblems, StoreLocation};
use storage::LocalStore;
use ui::test_support::draw_window;
use url::Url;
use workspace::Workspace;

/// An obviously fake cookie value (`.agents/rules/tests.md`).
const SID_VALUE: &str = "token-REDACTED-example";

#[gpui::test(iterations = 40)]
fn test_a_cleared_cookie_stays_cleared_across_a_restart(cx: &mut TestAppContext) {
    let store = shared_store();
    let cx = boot_app(cx, Arc::clone(&store));
    let jar = app_cookie_jar(cx);

    // First change: the server sets a session cookie; the wire saves the jar.
    jar.lock()
        .expect("the jar lock is free")
        .store_response_cookies(
            &Url::parse("https://example.test/").expect("the fixture URL parses"),
            &[format!("sid={SID_VALUE}; Path=/")],
        );
    notify_completed_send(cx);

    // Second change, before the first save has run: the server clears it (a logout).
    jar.lock()
        .expect("the jar lock is free")
        .clear_domain("example.test");
    notify_completed_send(cx);
    cx.run_until_parked();

    // The relaunch builds a fresh jar and loads what the store kept.
    cx.update(|_window, cx| {
        pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store)))
    });
    cx.run_until_parked();

    let names = app_cookie_jar(cx)
        .lock()
        .expect("the jar lock is free")
        .list()
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert!(
        names.is_empty(),
        "the cookie the server cleared must not come back after a restart: {names:?}"
    );
}

/// What a completed send does to the save wire: one Console entry, i.e. one change
/// notification of the `ConsoleStore` global the wire observes.
fn notify_completed_send(cx: &mut VisualTestContext) {
    cx.update(|_window, cx| {
        cx.global_mut::<ConsoleStore>();
    });
}

fn boot_app(cx: &mut TestAppContext, store: Arc<Mutex<LocalStore>>) -> &mut VisualTestContext {
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(store)));
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|_window, cx| StartupProblems::attach_workspace(&workspace, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.run_until_parked();
    draw_window(cx);
    cx
}

fn app_cookie_jar(cx: &VisualTestContext) -> Arc<Mutex<CookieJar>> {
    cx.read(|cx| {
        cx.try_global::<AppCookieJar>()
            .expect("the app installed its cookie jar")
            .0
            .clone()
    })
}

fn shared_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}
