//! The Auth tab in two columns (mockup section 8): a help paragraph on the left that
//! says how the type is sent and where secret values live, and on the right the
//! label/field grid with exactly the fields the chosen type has.

use api_model::{Auth, HttpRequest};
use gpui::{Entity, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use super::{AUTH_HELP_SELECTOR, AuthTab, AuthType};

fn open(cx: &mut TestAppContext) -> (Entity<AuthTab>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(text_input::init);
    let mut request: HttpRequest = crate::new_request::fresh_request();
    request.auth = Auth::None;
    let (tab, cx) = cx.add_window_view(|window, cx| AuthTab::new(&request, window, cx));
    draw_window(cx);
    (tab, cx)
}

fn choose(tab: &Entity<AuthTab>, auth_type: AuthType, cx: &mut VisualTestContext) {
    cx.update(|_, cx| tab.update(cx, |tab, cx| tab.set_auth_type(auth_type, cx)));
    draw_window(cx);
}

fn shown(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.debug_bounds(selector).is_some()
}

#[gpui::test]
fn test_api_key_auth_shows_key_value_and_add_to_beside_the_help(cx: &mut TestAppContext) {
    let (tab, cx) = open(cx);
    choose(&tab, AuthType::ApiKey, cx);

    for field in ["auth-control-key", "auth-control-value", "auth-control-location"] {
        assert!(shown(cx, field), "API key auth shows {field}");
    }
    assert!(!shown(cx, "auth-control-username"));
    let help = cx.debug_bounds(AUTH_HELP_SELECTOR).expect("the help column is drawn");
    let key = cx.debug_bounds("auth-control-key").expect("the key field");
    assert!(
        help.right() <= key.left(),
        "the help paragraph is the left column, the fields the right: {help:?} vs {key:?}"
    );
    assert!(
        tab.read_with(cx, |tab, _| tab.help_text().contains("OS keychain")),
        "the help says secret values live in the OS keychain"
    );
}

#[gpui::test]
fn test_each_auth_type_shows_only_its_own_fields(cx: &mut TestAppContext) {
    let (tab, cx) = open(cx);

    choose(&tab, AuthType::Basic, cx);
    assert!(shown(cx, "auth-control-username") && shown(cx, "auth-control-password"));
    assert!(!shown(cx, "auth-control-token") && !shown(cx, "auth-control-key"));

    choose(&tab, AuthType::Bearer, cx);
    assert!(shown(cx, "auth-control-token"));
    assert!(!shown(cx, "auth-control-username") && !shown(cx, "auth-control-key"));

    choose(&tab, AuthType::None, cx);
    for field in ["auth-control-token", "auth-control-username", "auth-control-key"] {
        assert!(!shown(cx, field), "no auth has no {field}");
    }
    assert!(shown(cx, "auth-control-type"), "the type picker stays");
}
