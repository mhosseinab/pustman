//! The order a cancelled send reports itself in: [`SendEvent::Cancelled`] reaches
//! subscribers only after the cancelled history row is stored (REQ-HIST-01 records every
//! send). The Console entry the event becomes and the History panel's re-read of the
//! database both run on it, so neither may land before the row exists.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_model::{Auth, Body, HttpRequest, ItemId, Method};
use gpui::{AppContext, TestAppContext};
use secrets::MemorySecretStore;
use storage::LocalStore;
use variables::{ScopeKind, ScopeStack};

use crate::{
    FakeTransport, HistoryRecorder, HttpClient, PipelineError, SendEvent, SendInput, TransportError,
};

/// The workspace every test records into.
const WORKSPACE: &str = "workspace-under-test";

/// When a cancelled send has a history row to write, subscribers see
/// [`SendEvent::Cancelled`] only after that row is stored: the re-read it triggers must
/// never run before the row exists and leave the reader stale.
#[gpui::test(iterations = 20)]
async fn test_cancelled_event_fires_after_the_cancelled_row_is_stored(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(30), 200, "late");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let store = open_store();
    client.update(cx, |client, _| {
        client.set_history(HistoryRecorder::new(store.clone(), WORKSPACE));
    });

    // What the subscriber could read from the store when `Cancelled` arrived — `None`
    // while no `Cancelled` has arrived at all.
    let cancelled_row_visible = Rc::new(RefCell::new(None));
    let seen = Rc::clone(&cancelled_row_visible);
    let watched = Arc::clone(&store);
    let _subscription = cx.update(|cx| {
        cx.subscribe(&client, move |_, event: &SendEvent, _| {
            if *event == SendEvent::Cancelled {
                *seen.borrow_mut() = Some(cancelled_row_stored(&watched));
            }
        })
    });

    let waiter = client.update(cx, |client, cx| client.send_request(send_input(), cx));
    client.update(cx, |client, cx| client.cancel(cx));
    cx.executor().run_until_parked();
    let outcome = waiter.await;
    assert!(
        matches!(
            outcome,
            Err(PipelineError::Transport(TransportError::Cancelled))
        ),
        "the cancelled send resolves as cancelled"
    );
    assert_eq!(
        *cancelled_row_visible.borrow(),
        Some(true),
        "the cancelled row must be stored before subscribers see `Cancelled`"
    );
}

/// Whether the store already holds the cancelled send's row.
fn cancelled_row_stored(store: &Mutex<LocalStore>) -> bool {
    let rows = store
        .lock()
        .expect("the lock is free")
        .history()
        .search(WORKSPACE, "")
        .expect("the rows list");
    rows.iter().any(|row| {
        row.response_summary_json
            .as_ref()
            .is_some_and(|summary| summary.get("cancelled").is_some())
    })
}

/// The in-memory local store the recording goes through.
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// One GET to `https://example.test/users` with no params, no body and no own auth.
fn plain_request() -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "cancel order test".to_string(),
        method: Method::Get,
        url: "https://example.test/users".to_string(),
        params: Vec::new(),
        path_variables: Vec::new(),
        headers: Vec::new(),
        body: Body::None,
        auth: Auth::Inherit,
        settings: None,
        examples: Vec::new(),
        extra: BTreeMap::new(),
    }
}

/// Everything [`HttpClient::send_request`] needs for one send: no variables, no secrets.
fn send_input() -> SendInput {
    SendInput {
        request: plain_request(),
        ancestors: Vec::new(),
        scopes: ScopeStack::new(),
        secrets: Arc::new(MemorySecretStore::new()),
        secret_key: Arc::new(|_kind: ScopeKind, _name: &str| None),
    }
}
