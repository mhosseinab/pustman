//! Closing a request tab cancels its send (S38 R2 review, MEDIUM 1): a send left running
//! after its tab is gone would still write its history row, while its Console entry —
//! written by the tab when the response lands — would never appear. Cancelling on release
//! gives the send one cancelled outcome, and leaves the other tabs' sends alone.

use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext, TestAppContext};
use http_engine::{FakeTransport, HttpClient, SendEvent};

use super::set_http_client;
use crate::RequestEditor;

fn request(url: &str) -> api_model::HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = url.to_string();
    request
}

#[gpui::test]
fn test_closing_a_tab_cancels_its_send_and_only_its_send(cx: &mut TestAppContext) {
    let transport = FakeTransport::new(cx.executor());
    transport.push_response(Duration::from_secs(5), 200, "closed tab");
    transport.push_response(Duration::from_secs(5), 201, "open tab");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(ui::test_support::init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        console::init(cx);
        set_http_client(client.clone(), cx);
    });
    let cancellations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let _watch = cx.update(|cx| {
        let cancellations = Arc::clone(&cancellations);
        // Every `SendEvent` the client emits reports a cancelled send.
        cx.subscribe(&client, move |_, _event: &SendEvent, _| {
            cancellations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })
    });

    let (kept, cx) = cx.add_window_view(|window, cx| {
        RequestEditor::new(request("https://example.test/open"), window, cx)
    });
    let closed = cx.update(|window, cx| {
        cx.new(|cx| RequestEditor::new(request("https://example.test/closed"), window, cx))
    });
    closed.update(cx, |editor, cx| editor.start_send(cx));
    kept.update(cx, |editor, cx| editor.start_send(cx));
    cx.run_until_parked();

    // The tab closes mid-send: its last handle goes away.
    let released = closed.downgrade();
    drop(closed);
    // GPUI releases a dropped entity when it next flushes effects, as closing a pane does.
    cx.update(|_window, _cx| {});
    cx.run_until_parked();
    assert!(released.upgrade().is_none(), "the closed tab was released");

    assert_eq!(
        transport.dropped(),
        1,
        "the closed tab's send was cancelled"
    );
    assert_eq!(
        cancellations.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the closed tab's send reports exactly one cancellation"
    );
    assert!(
        cx.read(|cx| kept.read(cx).is_sending()),
        "the open tab's send keeps running"
    );
    cx.executor().advance_clock(Duration::from_secs(6));
    cx.run_until_parked();
    assert_eq!(
        cx.read(|cx| kept.read(cx).status_line().map(|line| line.status)),
        Some(201),
        "the open tab still gets its own response"
    );
}
