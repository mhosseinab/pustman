//! The status code's color comes from the theme's HTTP class tokens, not from a literal:
//! every class maps to its token, and switching the theme switches the color with it. A
//! hard-coded hue would keep one theme's value and fail the second half of this test.

use gpui::TestAppContext;
use theme::{ActiveTheme, Appearance, ThemeStore};
use ui::test_support::init_test;

use super::status_class_color;

#[gpui::test]
fn status_class_colors_from_theme(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(|cx| {
        let status = cx.theme().status();
        assert_eq!(status_class_color(103, cx), status.http_informational);
        assert_eq!(status_class_color(204, cx), status.http_success);
        assert_eq!(status_class_color(301, cx), status.http_redirect);
        assert_eq!(status_class_color(404, cx), status.http_client_error);
        assert_eq!(status_class_color(503, cx), status.http_server_error);
    });

    cx.update(|cx| ThemeStore::set_system_appearance(cx, Appearance::Dark));
    let (dark_code, dark_token) = cx.update(|cx| {
        (
            status_class_color(204, cx),
            cx.theme().status().http_success,
        )
    });
    cx.update(|cx| ThemeStore::set_system_appearance(cx, Appearance::Light));
    let (light_code, light_token) = cx.update(|cx| {
        (
            status_class_color(204, cx),
            cx.theme().status().http_success,
        )
    });
    assert_eq!(
        dark_code, dark_token,
        "the 2xx color is the dark theme's token"
    );
    assert_eq!(
        light_code, light_token,
        "the 2xx color is the light theme's token"
    );
    assert_ne!(
        dark_code, light_code,
        "the 2xx color follows the theme; it is not a hard-coded literal"
    );
}
