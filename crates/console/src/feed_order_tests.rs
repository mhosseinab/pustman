//! What a cancellation entry names, however entries land in order (S33o): a cancelled
//! send's event is emitted only after its history row is written, so a replaced send's
//! "Cancelled" entry can land after the new send's entry — and it must still name its
//! own send (REQ-CON-01, REQ-HTTP-12).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use api_model::{Auth, Body, HttpRequest, ItemId, Method};
use gpui::{AppContext, TestAppContext};
use http_engine::{
    FakeTransport, HistoryRecorder, HttpClient, PipelineError, RequestBody, ResolvedRequest,
    SendInput, SendSettings, TransportError,
};
use secrets::MemorySecretStore;
use storage::LocalStore;
use variables::{ScopeKind, ScopeStack};

use crate::{ConsoleStore, record_error, watch};

/// The workspace every test records history into.
const WORKSPACE: &str = "workspace-under-test";

/// A replaced send's "Cancelled" entry lands after the new send's entry — the event that
/// becomes it is emitted only after the cancelled history row is written (S33o) — and
/// names its own send whatever the order entries land in.
#[gpui::test]
async fn test_a_replaced_sends_cancelled_entry_names_its_own_send_when_it_lands_last(
    cx: &mut TestAppContext,
) {
    let transport = FakeTransport::new(cx.executor());
    // A never answers; B fails at once — the fast-failing replace of the bug.
    transport.push_response(Duration::from_secs(30), 200, "late");
    transport.push_error(
        Duration::ZERO,
        TransportError::Dns {
            host: "b.test".to_string(),
        },
    );
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    client.update(cx, |client, _| {
        client.set_history(HistoryRecorder::new(open_store(), WORKSPACE));
    });
    init_console(cx);
    let _subscription = cx.update(|cx| watch(&client, cx));

    let _a = client.update(cx, |client, cx| {
        client.send_request(send_input(plain_request("https://a.test/users")), cx)
    });
    let b_request = plain_request("https://b.test/users");
    let _b = client.update(cx, |client, cx| {
        client.send_request(send_input(b_request.clone()), cx)
    });

    // B is the fast-failing replace: the caller records its entry the moment its task
    // resolves. Recording it here, before the scheduler runs anything, pins the order
    // this bug is about: A's cancellation is emitted only after its history row is
    // written, and that write cannot run until the scheduler does.
    let error = PipelineError::Transport(TransportError::Dns {
        host: "b.test".to_string(),
    });
    cx.update(|cx| record_error(Some(&b_request), &error, cx));
    cx.run_until_parked();

    cx.read(|cx| {
        let entries = cx.global::<ConsoleStore>().entries();
        assert_eq!(
            entries.len(),
            2,
            "B's failure lands one entry and A's cancellation lands one"
        );
        let failure = entries.front().expect("B's failure entry just recorded");
        let cancelled = entries.back().expect("A's cancelled entry just recorded");
        assert_eq!(
            cancelled.raw_response, "Cancelled",
            "the late entry is still the cancellation"
        );
        assert_eq!(failure.url, "https://b.test/users");
        assert_eq!(
            cancelled.method,
            Some(Method::Get),
            "the Cancelled entry names its own send's method"
        );
        assert_eq!(
            cancelled.url, "https://a.test/users",
            "the Cancelled entry names its own send's URL, whatever the order entries land in"
        );
    });
}

/// A cancelled send's entry names the request it dropped: the send that records no
/// history emits its cancellation at once, and its entry must name the send too.
#[gpui::test]
fn test_a_cancelled_send_names_the_request_it_dropped(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(30), 200, "late");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    init_console(cx);
    let _subscription = cx.update(|cx| watch(&client, cx));

    let request = ResolvedRequest::new(
        Method::Get,
        "https://a.test/never-sent",
        Vec::new(),
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("the fixture request URL parses");
    let _in_flight = client.update(cx, |client, cx| client.send(request, cx));
    client.update(cx, |client, cx| client.cancel(cx));
    cx.run_until_parked();

    cx.read(|cx| {
        let entries = cx.global::<ConsoleStore>().entries();
        assert_eq!(
            entries.len(),
            1,
            "a cancelled send lands exactly one Console entry"
        );
        let entry = entries.front().expect("the one entry just recorded");
        assert_eq!(entry.raw_response, "Cancelled");
        assert_eq!(entry.method, Some(Method::Get));
        assert_eq!(entry.url, "https://a.test/never-sent");
    });
}

/// Install the settings and the [`ConsoleStore`] global the feed pushes into.
fn init_console(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let init = settings::SettingsStore::init(cx, None);
        assert_eq!(init, Ok(()), "the store starts from the bundled defaults");
        crate::init(cx);
    });
}

/// The in-memory local store the history recording goes through.
fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

/// One GET to `url` with no params, no body and no own auth.
fn plain_request(url: &str) -> HttpRequest {
    HttpRequest {
        id: ItemId::new(),
        name: "feed order test".to_string(),
        method: Method::Get,
        url: url.to_string(),
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
fn send_input(request: HttpRequest) -> SendInput {
    SendInput {
        request,
        ancestors: Vec::new(),
        scopes: ScopeStack::new(),
        secrets: Arc::new(MemorySecretStore::new()),
        secret_key: Arc::new(|_kind: ScopeKind, _name: &str| None),
    }
}
