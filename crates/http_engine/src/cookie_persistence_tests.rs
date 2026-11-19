//! The cookie jar's persistence through `storage`'s `cookies` table (REQ-WSP-06): the
//! save-and-load round trip, the replace-on-save semantics, per-workspace isolation,
//! and the rows the loader drops. Fixture values are obviously fake
//! (`.agents/rules/tests.md`).

use std::sync::{Arc, Mutex};

use cookie_store::{Cookie as StoreCookie, CookieExpiration, CookieStore, RawCookie};
use gpui::TestAppContext;
use rusqlite::{Connection as SqliteConnection, params};
use storage::{Cookie as CookieRow, LocalStore};
use time::{Duration, OffsetDateTime};
use url::Url;

use crate::cookie_persistence::{load_jar, save_jar};
use crate::cookies::CookieJar;

/// Obviously fake cookie values (`.agents/rules/tests.md`).
const VALUE: &str = "token-REDACTED-example";
const OTHER_VALUE: &str = "token-REDACTED-other";
const THIRD_VALUE: &str = "token-REDACTED-third";
const WORKSPACE: &str = "workspace-under-test";
const OTHER_WORKSPACE: &str = "other-workspace";

fn url(text: &str) -> Url {
    Url::parse(text).expect("a valid URL")
}

/// The local store the persistence functions take, in memory and migrated
/// (`LocalStore::open_in_memory` + `migrate`, as the driver asked).
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// The local store the tampered-row tests need, with a raw-SQL connection beside it.
/// A row whose `attributes_json` is not JSON at all cannot be written through `storage`'s
/// API — its `insert` always serializes a `serde_json::Value` — so the tampering goes in
/// as raw SQL over a second connection to the same database file. The handle the tests
/// read and load through is the same one the other tests use.
fn open_store_with_raw_access() -> (Arc<Mutex<LocalStore>>, SqliteConnection, tempfile::TempDir) {
    let directory = tempfile::tempdir().expect("the temp directory opens");
    let path = directory.path().join("local-store.sqlite");
    let mut store = LocalStore::open(&path).expect("the local store opens");
    store.migrate().expect("the schema migrates");
    let raw = SqliteConnection::open(&path).expect("the raw connection opens");
    (Arc::new(Mutex::new(store)), raw, directory)
}

/// What the `cookies` table holds for one workspace, read back the way `load_jar` does.
fn stored_rows(store: &Mutex<LocalStore>, workspace_id: &str) -> Vec<CookieRow> {
    store
        .lock()
        .expect("the lock is free")
        .cookies()
        .list(workspace_id)
        .expect("the rows list")
}

/// A row as `storage` holds it, serialized exactly as `save_jar` serializes a cookie.
fn insert_row(store: &Mutex<LocalStore>, workspace_id: &str, cookie: &StoreCookie<'static>) {
    let row = CookieRow {
        workspace_id: workspace_id.to_owned(),
        domain: String::from(&cookie.domain),
        path: String::from(&cookie.path),
        name: cookie.name().to_owned(),
        value: cookie.value().to_owned(),
        attributes_json: serde_json::to_value(cookie).expect("the cookie serializes"),
    };
    store
        .lock()
        .expect("the lock is free")
        .cookies()
        .insert(&row)
        .expect("the row inserts");
}

/// A cookie that expired long ago. Built directly because the jar's own API refuses
/// expired cookies — the persisted rows and the jar can still outlive one.
fn expired_cookie() -> StoreCookie<'static> {
    let raw = RawCookie::build(("gone", VALUE)).path("/").build();
    let mut cookie = StoreCookie::try_from_raw_cookie(&raw, &url("https://example.com/"))
        .expect("the cookie is valid")
        .into_owned();
    cookie.expires = CookieExpiration::AtUtc(OffsetDateTime::now_utc() - Duration::days(1));
    cookie
}

/// A jar survives the round trip: what the dialog lists is identical after save and
/// load, and a `Domain=` cookie is still a domain cookie — reloading must not narrow it
/// to a host-only one (fidelity), so a subdomain still gets it.
#[gpui::test]
async fn test_jar_persists_and_reloads_from_storage(cx: &mut TestAppContext) {
    let store = open_store();
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &url("https://example.com/api/v1"),
        &[
            format!("sid={VALUE}; Path=/api"),
            format!("region={OTHER_VALUE}; Domain=example.com; Path=/"),
            format!("theme={THIRD_VALUE}; Path=/; Expires=Wed, 21 Oct 2030 07:28:00 GMT"),
        ],
    );
    jar.store_response_cookies(
        &url("https://other.test/"),
        &[format!("other={THIRD_VALUE}; Path=/")],
    );

    save_jar(&cx.executor(), store.clone(), WORKSPACE.to_owned(), &jar)
        .await
        .expect("the jar saves");
    let loaded = load_jar(&cx.executor(), store, WORKSPACE.to_owned())
        .await
        .expect("the jar loads");

    assert_eq!(
        loaded.list(),
        jar.list(),
        "the dialog rows survive the round trip, expiry included"
    );
    let subdomain = url("https://api.example.com/api/v2");
    assert_eq!(
        loaded.cookie_header_for(&subdomain),
        jar.cookie_header_for(&subdomain),
        "the reload restores the same scoping"
    );
    let header = loaded
        .cookie_header_for(&subdomain)
        .expect("the domain cookie matches a subdomain");
    assert!(
        header.contains(&format!("region={OTHER_VALUE}")),
        "the `Domain=example.com` cookie is still a domain cookie, not host-only"
    );
}

/// Save is a replace: a cookie deleted from the jar is gone from the table after the
/// next save, not merely absent from what loads.
#[gpui::test]
async fn test_deleted_cookie_is_gone_from_storage_after_the_next_save(cx: &mut TestAppContext) {
    let store = open_store();
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &url("https://example.com/"),
        &[
            format!("sid={VALUE}; Path=/"),
            format!("gone={OTHER_VALUE}; Path=/"),
        ],
    );
    save_jar(&cx.executor(), store.clone(), WORKSPACE.to_owned(), &jar)
        .await
        .expect("the jar saves");

    jar.delete("example.com", "/", "gone")
        .expect("the cookie is listed");
    save_jar(&cx.executor(), store.clone(), WORKSPACE.to_owned(), &jar)
        .await
        .expect("the jar saves again");

    let rows = stored_rows(&store, WORKSPACE);
    assert_eq!(
        rows.len(),
        1,
        "the deleted cookie is deleted from the table"
    );
    assert_eq!(rows[0].name, "sid");
}

/// Each workspace's rows are its own: a save never touches another workspace's rows,
/// and a load never returns another workspace's cookies.
#[gpui::test]
async fn test_save_and_load_never_touch_another_workspace(cx: &mut TestAppContext) {
    let store = open_store();
    let mut mine = CookieJar::new();
    mine.store_response_cookies(
        &url("https://example.com/"),
        &[format!("sid={VALUE}; Path=/")],
    );
    let mut theirs = CookieJar::new();
    theirs.store_response_cookies(
        &url("https://other.test/"),
        &[format!("other={OTHER_VALUE}; Path=/")],
    );
    save_jar(&cx.executor(), store.clone(), WORKSPACE.to_owned(), &mine)
        .await
        .expect("the jar saves");
    save_jar(
        &cx.executor(),
        store.clone(),
        OTHER_WORKSPACE.to_owned(),
        &theirs,
    )
    .await
    .expect("the other jar saves");

    // An emptied jar replaces its own rows away — and only its own.
    mine.delete("example.com", "/", "sid")
        .expect("the cookie is listed");
    save_jar(&cx.executor(), store.clone(), WORKSPACE.to_owned(), &mine)
        .await
        .expect("the emptied jar saves");

    assert!(
        stored_rows(&store, WORKSPACE).is_empty(),
        "my rows are replaced away"
    );
    assert_eq!(
        stored_rows(&store, OTHER_WORKSPACE).len(),
        1,
        "the other workspace's rows are untouched"
    );
    let loaded = load_jar(&cx.executor(), store, OTHER_WORKSPACE.to_owned())
        .await
        .expect("the jar loads");
    assert_eq!(
        loaded.list(),
        theirs.list(),
        "load returns its own workspace only"
    );
}

/// Expired cookies are neither written nor reloaded: the table holds the jar's
/// unexpired cookies, and load drops what expired since its row was written.
#[gpui::test]
async fn test_expired_cookie_is_neither_written_nor_reloaded(cx: &mut TestAppContext) {
    let store = open_store();
    let expired = expired_cookie();

    // Not written: a jar still holding a cookie that expired since it was stored saves
    // nothing for it.
    let holding_expired = CookieStore::from_cookies(
        vec![Ok::<_, std::convert::Infallible>(expired.clone())],
        true,
    )
    .expect("the iterator is infallible");
    let jar = CookieJar::from_store(holding_expired);
    save_jar(&cx.executor(), store.clone(), WORKSPACE.to_owned(), &jar)
        .await
        .expect("the jar saves");
    assert!(
        stored_rows(&store, WORKSPACE).is_empty(),
        "the expired cookie is not written"
    );

    // Not reloaded: a row written before the cookie expired is dropped on load.
    insert_row(&store, WORKSPACE, &expired);
    let loaded = load_jar(&cx.executor(), store, WORKSPACE.to_owned())
        .await
        .expect("the jar loads");
    assert_eq!(
        loaded.list(),
        Vec::new(),
        "the expired cookie is dropped on load"
    );
}

/// A row whose attributes parse as JSON but do not deserialize into a cookie is skipped
/// on load — the table is local, untrusted data — and the rest of the jar still loads.
/// A row that is not JSON at all is not skippable: `storage`'s `list` fails the whole
/// read, and `test_non_json_cookie_row_fails_the_load_with_fixed_text` pins the
/// fixed-text error that produces.
#[gpui::test]
async fn test_row_that_does_not_deserialize_is_skipped_on_load(cx: &mut TestAppContext) {
    let store = open_store();
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &url("https://example.com/"),
        &[format!("sid={VALUE}; Path=/")],
    );
    save_jar(&cx.executor(), store.clone(), WORKSPACE.to_owned(), &jar)
        .await
        .expect("the jar saves");

    let broken_row = CookieRow {
        workspace_id: WORKSPACE.to_owned(),
        domain: String::from("broken.test"),
        path: String::from("/"),
        name: String::from("broken"),
        value: String::new(),
        attributes_json: serde_json::json!({"not": "a cookie"}),
    };
    store
        .lock()
        .expect("the lock is free")
        .cookies()
        .insert(&broken_row)
        .expect("the row inserts");

    let loaded = load_jar(&cx.executor(), store, WORKSPACE.to_owned())
        .await
        .expect("the jar loads");
    assert_eq!(
        loaded.list(),
        jar.list(),
        "the broken row is skipped and the rest loads"
    );
}

/// A row whose `attributes_json` is not JSON at all is the case `storage` cannot skip:
/// `CookiesRepo::list` parses a workspace's rows in one read, so the load fails as a
/// whole — with one fixed-text error, never the tampered row and never serde's line and
/// column (PRD §7). Lifting this needs a per-row-tolerant read in `storage`.
#[gpui::test]
async fn test_non_json_cookie_row_fails_the_load_with_fixed_text(cx: &mut TestAppContext) {
    let (store, raw, _temp_directory) = open_store_with_raw_access();
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &url("https://example.com/"),
        &[format!("sid={VALUE}; Path=/")],
    );
    save_jar(&cx.executor(), store.clone(), WORKSPACE.to_owned(), &jar)
        .await
        .expect("the good row saves");

    // The tampering raw SQL can express and `storage`'s API cannot: attributes that are
    // not a JSON document at all.
    raw.execute(
        "INSERT INTO cookies (workspace_id, domain, path, name, value, attributes_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            WORKSPACE,
            "tampered.invalid",
            "/",
            "tampered-row",
            VALUE,
            "not json at all"
        ],
    )
    .expect("the tampered row inserts");

    let error = load_jar(&cx.executor(), store, WORKSPACE.to_owned())
        .await
        .expect_err("a non-JSON row fails the whole load");
    let message = error.to_string();
    assert_eq!(
        message, "The saved cookies could not be read",
        "the load fails with the one fixed-text read error"
    );
    for fragment in [
        "not json at all",
        "tampered.invalid",
        "tampered-row",
        VALUE,
        "line",
        "column",
    ] {
        assert!(
            !message.contains(fragment),
            "the error text must not contain `{fragment}` from the tampered row or serde"
        );
    }
}

/// A jar reloaded from storage still rejects public-suffix cookies: the reload goes
/// through `CookieStore::from_cookies`, so unless `from_store` attaches the same vendored
/// list a fresh jar carries, `Domain=com` would reopen the supercookie hole after every
/// restart (S15a's RFC 6265 §5.3 step 5 rejection must survive the round trip).
#[gpui::test]
async fn test_reloaded_jar_still_rejects_public_suffix_cookies(cx: &mut TestAppContext) {
    let store = open_store();
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &url("https://example.com/"),
        &[format!("sid={VALUE}; Path=/")],
    );
    save_jar(&cx.executor(), store.clone(), WORKSPACE.to_owned(), &jar)
        .await
        .expect("the jar saves");
    let mut loaded = load_jar(&cx.executor(), store, WORKSPACE.to_owned())
        .await
        .expect("the jar loads");

    loaded.store_response_cookies(
        &url("https://api.example.com/"),
        &[format!("super={OTHER_VALUE}; Domain=com")],
    );
    loaded.store_response_cookies(
        &url("https://example.co.uk/"),
        &[format!("public={THIRD_VALUE}; Domain=co.uk")],
    );
    loaded.store_response_cookies(
        &url("https://api.example.com/"),
        &[format!("legit={THIRD_VALUE}; Domain=example.com; Path=/")],
    );

    let entries = loaded.list();
    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
    assert!(
        !names.contains(&"super"),
        "`Domain=com` is rejected on a reloaded jar, not just a fresh one"
    );
    assert!(
        !names.contains(&"public"),
        "`Domain=co.uk` is rejected on a reloaded jar, not just a fresh one"
    );
    assert!(
        names.contains(&"legit"),
        "a `Domain=example.com` cookie from api.example.com is still kept"
    );
    assert!(
        loaded
            .cookie_header_for(&url("https://www.example.com/"))
            .is_some_and(|header| header.contains(&format!("legit={THIRD_VALUE}"))),
        "the kept cookie is still a domain cookie reaching its siblings"
    );
}

/// A stored cookie whose domain is a public suffix is dropped on load: the table is
/// tamperable local data, and `CookieStore::from_cookies` runs no public-suffix check, so
/// a `Domain=com` row would otherwise reload as a supercookie for every `*.com` host
/// (RFC 6265 §5.3 step 5 must hold for rows too, not just for `Set-Cookie`).
#[gpui::test]
async fn test_stored_public_suffix_cookie_is_dropped_on_load(cx: &mut TestAppContext) {
    let store = open_store();
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &url("https://example.com/"),
        &[format!("sid={VALUE}; Path=/")],
    );
    save_jar(&cx.executor(), store.clone(), WORKSPACE.to_owned(), &jar)
        .await
        .expect("the good row saves");

    // A cookie the jar could never accept (`Domain=com`), serialized into a row the way
    // `save_jar` serializes one — exactly what a tampered table holds.
    let raw = RawCookie::build(("super", OTHER_VALUE))
        .domain("com")
        .path("/")
        .build();
    let supercookie = StoreCookie::try_from_raw_cookie(&raw, &url("https://api.example.com/"))
        .expect("the crafted cookie is valid")
        .into_owned();
    insert_row(&store, WORKSPACE, &supercookie);

    let loaded = load_jar(&cx.executor(), store, WORKSPACE.to_owned())
        .await
        .expect("the jar loads");
    assert_eq!(
        loaded.list(),
        jar.list(),
        "the stored public-suffix cookie is dropped and only the good row lists"
    );
    assert!(
        loaded
            .cookie_header_for(&url("https://www.example.com/"))
            .is_none(),
        "the dropped supercookie reaches no `*.com` host"
    );
}
