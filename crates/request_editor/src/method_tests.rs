//! The custom method entry: a typed token the method rule rejects is shown as an
//! inline error and the request keeps the method it had. The flow is driven the way
//! the dropdown drives it and the way a user does it: the entry point, a typed token
//! and Enter.

use api_model::Method;
use gpui::TestAppContext;
use ui::test_support::{draw_window, init_test, press_key};

use crate::url_bar::{CUSTOM_ERROR_SELECTOR, UrlBar};

#[gpui::test]
fn custom_method_rejects_invalid_token(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(|cx| {
        // The typing below needs the editing keys bound.
        text_input::init(cx);
    });
    let (url_bar, cx) =
        cx.add_window_view(|window, cx| UrlBar::new(Method::Get, String::new(), window, cx));
    draw_window(cx);

    // What the dropdown's "Custom Method…" entry runs.
    cx.update(|window, cx| {
        url_bar.update(cx, |bar, cx| bar.begin_custom_method(window, cx));
    });
    draw_window(cx);

    cx.simulate_input("bad method");
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        cx.read(|cx| url_bar.read_with(cx, |bar, _| bar.method().clone())),
        Method::Get,
        "a token outside the RFC 9110 token set is not applied"
    );
    let message = cx.read(|cx| url_bar.read_with(cx, |bar, _| bar.custom_error().cloned()));
    assert!(
        message
            .as_ref()
            .is_some_and(|message| message.contains("bad method")),
        "the rejection names the token that was typed: {message:?}"
    );
    assert!(
        cx.debug_bounds(CUSTOM_ERROR_SELECTOR).is_some(),
        "the rejection is shown as an inline error"
    );
}
