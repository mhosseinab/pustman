//! The method picker is colour-coded (#217): the verb in the method control and in its
//! dropdown draws in the theme's method token — the colour History rows already use —
//! and a verb with no token (a custom method) falls back to muted text. The verb's text
//! stays beside the colour (`docs/postman_redesign_pack/01_specs/design_system.md`,
//! Theme roles: "Method labels always include text … HTTP method color alone carries no
//! meaning").
//!
//! `Decision:` the colour is asserted through [`UrlBar::method_label_color`] — the value
//! `render` hands the label and its menu entries — because GPUI's test support exposes
//! layout (`debug_bounds`) and painted quads but not text colours: glyphs paint as
//! atlas sprites and the scene's sprite list sits behind gpui's `bench-support`
//! feature. The accessor is what the control paints, so what a test asserts here is
//! what the user sees.

use api_model::Method;
use gpui::{App, TestAppContext};
use ui::prelude::*;
use ui::test_support::{draw_window, init_test, press_key};

use crate::url_bar::UrlBar;

/// The theme token each standard verb draws in (`theme::StatusColors`), spelled out
/// here rather than through the production mapping: a test states the requirement, it
/// does not repeat the implementation.
fn expected_color(method: &Method, cx: &App) -> Color {
    let status = cx.theme().status();
    let token = match method {
        Method::Get => status.method_get,
        Method::Post => status.method_post,
        Method::Put => status.method_put,
        Method::Patch => status.method_patch,
        Method::Delete => status.method_delete,
        Method::Head => status.method_head,
        Method::Options => status.method_options,
        // Trace, Connect and custom verbs have no method token: muted text.
        _ => return Color::Muted,
    };
    Color::Custom(token)
}

/// The colour the method control draws `method` in.
fn label_color(method: Method, cx: &mut TestAppContext) -> Color {
    let (bar, cx) = cx.add_window_view(|window, cx| UrlBar::new(method, String::new(), window, cx));
    draw_window(cx);
    cx.read(|cx| bar.read_with(cx, |bar, cx| bar.method_label_color(cx)))
}

#[gpui::test]
fn every_method_draws_in_its_theme_token(cx: &mut TestAppContext) {
    cx.update(init_test);
    for method in crate::method::STANDARD_METHODS {
        let color = label_color(method.clone(), cx);
        let expected = cx.read(|cx| expected_color(&method, cx));
        assert_eq!(
            color,
            expected,
            "the method control colours {} from its theme token",
            method.as_str()
        );
    }
    assert_eq!(
        label_color(Method::Custom(String::from("PROPFIND")), cx),
        Color::Muted,
        "a verb with no theme token falls back to muted text"
    );
}

#[gpui::test]
fn the_colour_follows_the_method_the_user_applies(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
    });
    let (bar, cx) =
        cx.add_window_view(|window, cx| UrlBar::new(Method::Get, String::new(), window, cx));
    draw_window(cx);
    let before = cx.read(|cx| bar.read_with(cx, |bar, cx| bar.method_label_color(cx)));

    // The custom-method entry the dropdown's "Custom Method…" item runs.
    cx.update(|window, cx| {
        bar.update(cx, |bar, cx| bar.begin_custom_method(window, cx));
    });
    cx.simulate_input("PATCH");
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);

    let color = cx.read(|cx| bar.read_with(cx, |bar, cx| bar.method_label_color(cx)));
    let patch = cx.read(|cx| cx.theme().status().method_patch);
    assert_eq!(
        color,
        Color::Custom(patch),
        "applying a method recolours the control for it"
    );
    assert_ne!(
        color, before,
        "and the control no longer draws the colour of the method it replaced"
    );
}
