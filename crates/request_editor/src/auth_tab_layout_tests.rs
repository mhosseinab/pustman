//! The Auth tab reads as an aligned form, like the Settings tab: each field's name sits
//! beside its control in one name column, every control starts in one control column,
//! and the inherited-source line and type notes sit under the control they describe.

use api_model::{ApiKeyLocation, Auth, HttpRequest};
use gpui::{Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use crate::RequestEditor;

fn api_key_request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.auth = Auth::ApiKey {
        key: None,
        value: None,
        location: Some(ApiKeyLocation::Header),
        extra: Default::default(),
    };
    request
}

fn open_auth_tab(cx: &mut TestAppContext, request: HttpRequest) -> &mut VisualTestContext {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (_editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
    draw_window(cx);
    let tab = bounds("request-tab-auth", cx);
    cx.simulate_click(tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
    cx
}

fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is on screen"))
}

#[gpui::test]
fn test_api_key_fields_line_up_in_a_name_and_a_control_column(cx: &mut TestAppContext) {
    let cx = open_auth_tab(cx, api_key_request());
    let rows = [
        ("auth-label-type", "auth-control-type"),
        ("auth-label-key", "auth-control-key"),
        ("auth-label-value", "auth-control-value"),
        ("auth-label-location", "auth-control-location"),
    ];

    let name_left = bounds(rows[0].0, cx).left();
    let control_left = bounds(rows[0].1, cx).left();
    assert!(
        control_left > name_left,
        "the control column follows the name column"
    );
    for (label, control) in rows {
        let label_bounds = bounds(label, cx);
        let control_bounds = bounds(control, cx);
        assert_eq!(
            label_bounds.left(),
            name_left,
            "{label} starts the name column"
        );
        assert_eq!(
            control_bounds.left(),
            control_left,
            "{control} starts the control column"
        );
        let label_middle = label_bounds.center().y;
        assert!(
            label_middle >= control_bounds.top() && label_middle <= control_bounds.bottom(),
            "{label} sits beside {control}, not above it: {label_bounds:?} vs {control_bounds:?}"
        );
    }
}

#[gpui::test]
fn test_inherited_source_sits_under_the_type_control(cx: &mut TestAppContext) {
    let mut request = crate::new_request::fresh_request();
    request.auth = Auth::Inherit;
    let cx = open_auth_tab(cx, request);
    let control = bounds("auth-control-type", cx);
    let source = bounds(super::INHERITED_SOURCE_SELECTOR, cx);

    assert_eq!(
        source.left(),
        control.left(),
        "the inherited source line is aligned with the control it describes"
    );
    assert!(
        source.top() >= control.bottom(),
        "it sits under the control"
    );
}

#[gpui::test]
fn test_a_type_with_its_own_credentials_names_no_inherited_source(cx: &mut TestAppContext) {
    let cx = open_auth_tab(cx, api_key_request());

    assert!(
        cx.debug_bounds(super::INHERITED_SOURCE_SELECTOR).is_none(),
        "API key auth inherits nothing, so no inherited-source line is drawn"
    );
}

#[gpui::test]
fn test_a_field_input_fills_its_frame(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(text_input::init);
    let mut request = crate::new_request::fresh_request();
    request.auth = Auth::Bearer {
        token: None,
        extra: Default::default(),
    };
    let (_tab, cx) =
        cx.add_window_view(|window, cx| crate::auth_tab::AuthTab::new(&request, window, cx));
    draw_window(cx);

    let frame = bounds("auth-control-token", cx);
    // The bearer form has one text input: the token field.
    let input = bounds("text-input", cx);
    assert!(
        input.size.width >= frame.size.width * 0.9,
        "the token input spans its frame, not one character: {input:?} in {frame:?}"
    );
}
