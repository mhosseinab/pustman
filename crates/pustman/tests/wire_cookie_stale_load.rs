//! A relaunch before the first launch's cookie load lands must not erase the saved
//! cookies (#287, review of `84803b5`): the stale load belongs to a jar the app no longer
//! uses, so it must not mark saving ready for the new, still-empty jar. Every task is
//! stepped one at a time with a save trigger (a Console change) after each, and
//! `iterations` shuffles the order, so a stale load landing first is exercised.

#![allow(clippy::expect_used)]

use std::sync::{Arc, Mutex};

use console::ConsoleStore;
use gpui::TestAppContext;
use http_engine::{CookieJar, load_jar, save_jar};
use pustman::StoreLocation;
use storage::LocalStore;
use url::Url;

use pustman::WORKSPACE_ID;
/// An obviously fake cookie value (`.agents/rules/tests.md`).
const SID_VALUE: &str = "token-REDACTED-example";

#[gpui::test(iterations = 60)]
async fn test_a_relaunch_before_the_first_load_lands_keeps_the_saved_cookies(
    cx: &mut TestAppContext,
) {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    let store = Arc::new(Mutex::new(store));
    let mut seeded = CookieJar::new();
    seeded.store_response_cookies(
        &Url::parse("https://example.test/").expect("the fixture URL parses"),
        &[format!("sid={SID_VALUE}; Path=/")],
    );
    save_jar(
        &cx.background_executor,
        Arc::clone(&store),
        WORKSPACE_ID.into(),
        &seeded,
    )
    .await
    .expect("the seed cookie is saved");

    // The first launch: step until its store has opened, which starts its cookie load
    // into the first launch's jar — and relaunch before anything else runs, so that load
    // is still in flight when the second launch builds its own jar.
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store))));
    for _ in 0..300 {
        if cx.update(|cx| cx.has_global::<pustman::AppStore>()) {
            break;
        }
        if !cx.executor().tick() {
            break;
        }
    }
    assert!(
        cx.update(|cx| cx.has_global::<pustman::AppStore>()),
        "the first launch's store opened"
    );
    cx.update(|cx| pustman::init_with_store(cx, StoreLocation::Shared(Arc::clone(&store))));
    // A save trigger after every single task, so a save can run between the stale load
    // and the current one.
    // Bounded: every trigger queues a fresh save, so the queue never runs dry on its own.
    for _ in 0..300 {
        cx.update(|cx| {
            if cx.has_global::<ConsoleStore>() {
                cx.global_mut::<ConsoleStore>();
            }
        });
        if !cx.executor().tick() {
            break;
        }
    }
    cx.run_until_parked();

    let reloaded = load_jar(&cx.background_executor, store, WORKSPACE_ID.into())
        .await
        .expect("the jar reloads");
    let names: Vec<String> = reloaded
        .list()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(
        names,
        vec!["sid".to_owned()],
        "the saved cookie survives a relaunch that overtook the first load"
    );
}
