//! Every writer of a workspace's jar — the app's send-triggered saves and the Manage
//! Cookies dialog — persists through [`save_jar_latest`], which copies the jar at write
//! time under the store's lock (#287). Whichever save runs last writes the jar as it is
//! then, so an earlier save can never land a stale copy over a newer change, whatever
//! order the background executor runs them in.

use std::sync::{Arc, Mutex};

use gpui::TestAppContext;
use storage::LocalStore;
use url::Url;

use crate::cookie_persistence::{load_jar, save_jar_latest};
use crate::cookies::CookieJar;

const WORKSPACE: &str = "workspace-287";
/// An obviously fake cookie value (`.agents/rules/tests.md`).
const SID_VALUE: &str = "token-REDACTED-example";

fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

fn jar_with_session_cookie() -> Arc<Mutex<CookieJar>> {
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &Url::parse("https://example.test/").expect("the fixture URL parses"),
        &[format!("sid={SID_VALUE}; Path=/")],
    );
    Arc::new(Mutex::new(jar))
}

#[gpui::test(iterations = 40)]
async fn test_a_save_queued_before_a_clear_never_brings_the_cookie_back(cx: &mut TestAppContext) {
    let store = open_store();
    let jar = jar_with_session_cookie();
    let executor = cx.background_executor.clone();

    // A save is queued while the jar still holds the cookie (a send just completed)…
    let earlier = save_jar_latest(
        &executor,
        Arc::clone(&store),
        WORKSPACE.into(),
        Arc::clone(&jar),
    );
    // …then the cookie is cleared (the dialog's "Clear", or a server logout) and saved.
    jar.lock()
        .expect("the jar lock is free")
        .clear_domain("example.test");
    let later = save_jar_latest(
        &executor,
        Arc::clone(&store),
        WORKSPACE.into(),
        Arc::clone(&jar),
    );

    // The executor runs the two writes in a random order per iteration.
    let (earlier, later) = futures::join!(earlier, later);
    earlier.expect("the earlier save writes");
    later.expect("the later save writes");

    let reloaded = load_jar(&executor, store, WORKSPACE.into())
        .await
        .expect("the jar reloads");
    assert!(
        reloaded.list().is_empty(),
        "a cleared cookie must not be written back by an earlier save"
    );
}

#[gpui::test]
async fn test_a_save_writes_the_jar_as_it_is_when_the_write_runs(cx: &mut TestAppContext) {
    let store = open_store();
    let jar = Arc::new(Mutex::new(CookieJar::new()));
    let executor = cx.background_executor.clone();

    // Queued while the jar is empty; the cookie arrives before the write runs.
    let save = save_jar_latest(
        &executor,
        Arc::clone(&store),
        WORKSPACE.into(),
        Arc::clone(&jar),
    );
    jar.lock()
        .expect("the jar lock is free")
        .store_response_cookies(
            &Url::parse("https://example.test/").expect("the fixture URL parses"),
            &[format!("sid={SID_VALUE}; Path=/")],
        );
    save.await.expect("the save writes");

    let names = load_jar(&executor, store, WORKSPACE.into())
        .await
        .expect("the jar reloads")
        .list()
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec!["sid".to_owned()],
        "the write carries the latest jar"
    );
}
