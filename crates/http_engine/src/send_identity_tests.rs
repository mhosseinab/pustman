//! What a cancellation names about its send (REQ-HTTP-12, REQ-NFR-04): the identity the
//! [`SendEvent`] carries holds no secret-bearing text. A [`HttpClient::send`] request is
//! already resolved, and the resolved URL carries secret values — a webhook token lives
//! in the path (PRD §7) — so its identity keeps only the printable URL form, and
//! [`SendIdentity`]'s `Debug` prints no URL either.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use api_model::Method;
use gpui::{AppContext, Entity, Subscription, TestAppContext};

use crate::{
    FakeTransport, HttpClient, RequestBody, ResolvedRequest, SendEvent, SendIdentity, SendSettings,
};

/// The obviously-fake secret value the fixtures spread over the URL: it must reach no
/// identity, no Console entry and no `Debug` output.
const SECRET: &str = "token-REDACTED-example";

/// A cancelled [`HttpClient::send`] puts no resolved URL text on its event: whatever part
/// of the URL carried the secret, the identity keeps only the printable form.
#[gpui::test]
fn test_a_cancelled_send_puts_no_resolved_url_text_on_its_event(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(30), 200, "late");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    let (events, _subscription) = collect_events(cx, &client);

    let request = ResolvedRequest::new(
        Method::Get,
        &format!("https://{SECRET}:password@host.test/{SECRET}?apiKey={SECRET}#{SECRET}"),
        Vec::new(),
        RequestBody::None,
        SendSettings::default(),
    )
    .expect("the fixture request URL parses");
    let _in_flight = client.update(cx, |client, cx| client.send(request, cx));
    client.update(cx, |client, cx| client.cancel(cx));
    cx.run_until_parked();

    let events = events.borrow();
    let identity = events
        .first()
        .and_then(|event| event.send.as_ref())
        .expect("the cancellation names its send");
    assert_eq!(
        identity.method,
        Method::Get,
        "the method still names the send"
    );
    assert_eq!(
        identity.url, "https://<redacted>",
        "the resolved URL is secret-bearing text and is never captured — only its scheme"
    );
    assert!(
        !identity.url.contains(SECRET),
        "no secret value reaches the identity"
    );
}

/// [`SendIdentity`]'s `Debug` prints no URL: the URL may carry secret values (PRD §7),
/// which is why the capture reduces it in the first place.
#[test]
fn test_send_identity_debug_prints_no_url_values() {
    let identity = SendIdentity {
        method: Method::Get,
        url: format!("https://host.test/{SECRET}?apiKey={SECRET}"),
    };

    let printed = format!("{identity:?}");

    assert!(
        !printed.contains(SECRET),
        "`Debug` prints no URL values: {printed}"
    );
    assert!(printed.contains("Get"), "the method is what `Debug` shows");
}

/// Collect the [`SendEvent`]s `client` emits, for the life of the subscription.
fn collect_events(
    cx: &mut TestAppContext,
    client: &Entity<HttpClient>,
) -> (Rc<RefCell<Vec<SendEvent>>>, Subscription) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let subscription = cx.update(|cx| {
        let events = events.clone();
        cx.subscribe(client, move |_, event: &SendEvent, _| {
            events.borrow_mut().push(event.clone())
        })
    });
    (events, subscription)
}
