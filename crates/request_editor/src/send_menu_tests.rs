//! The split Send's chevron opens a menu of existing actions; each entry dispatches its
//! action to the editor — Copy as cURL fills the clipboard, Send starts the request —
//! and the chevron steps aside while a request is in flight.

use std::sync::Arc;

use gpui::{AppContext, Entity, Modifiers, TestAppContext, VisualTestContext};
use http_engine::{FakeTransport, HttpClient};
use ui::test_support::{draw_window, init_test};

use super::SEND_MENU_SELECTOR;
use crate::RequestEditor;
use crate::send::set_http_client;

fn open(cx: &mut TestAppContext) -> (Entity<RequestEditor>, FakeTransport, &mut VisualTestContext) {
    let transport = FakeTransport::new(cx.executor());
    // Answered late, so the request is still in flight when the test looks.
    transport.push_response(std::time::Duration::from_secs(60), 200, "ok");
    let client = cx.new(|_| HttpClient::new(Arc::new(transport.clone())));
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
        set_http_client(client, cx);
    });
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://example.test/menu");
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
    draw_window(cx);
    (editor, transport, cx)
}

fn open_menu(cx: &mut VisualTestContext) {
    let chevron = cx
        .debug_bounds(SEND_MENU_SELECTOR)
        .expect("the split Send's chevron is on screen");
    cx.simulate_click(chevron.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

fn click_entry(cx: &mut VisualTestContext, entry: &'static str) {
    let bounds = cx
        .debug_bounds(entry)
        .unwrap_or_else(|| panic!("the menu lists {entry}"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

#[gpui::test]
fn test_the_send_menu_copy_as_curl_entry_dispatches_copy_as_curl(cx: &mut TestAppContext) {
    let (_editor, _transport, cx) = open(cx);
    open_menu(cx);
    click_entry(cx, "Copy as cURL");

    let copied = cx
        .read_from_clipboard()
        .and_then(|item| item.text())
        .unwrap_or_default();
    assert!(
        copied.contains("https://example.test/menu"),
        "the entry ran request::CopyAsCurl: {copied}"
    );
    assert!(
        cx.debug_bounds("Copy as cURL").is_none(),
        "the menu closes once its entry ran"
    );
}

#[gpui::test]
fn test_the_send_menu_send_entry_dispatches_send(cx: &mut TestAppContext) {
    let (editor, transport, cx) = open(cx);
    open_menu(cx);
    click_entry(cx, "Send");

    assert_eq!(
        transport.requests().len(),
        1,
        "the entry ran request::Send: the request went out"
    );
    assert!(
        editor.read_with(cx, |editor, _| editor.is_sending()),
        "and is in flight"
    );
    assert!(
        cx.debug_bounds(SEND_MENU_SELECTOR).is_none(),
        "the chevron steps aside while the request is in flight"
    );
}
