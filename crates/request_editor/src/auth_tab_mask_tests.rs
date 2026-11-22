//! The Auth tab masks its credential fields (#178): the Basic password, the Bearer
//! token and the API key value show masks — what the painted line shapes and what the
//! platform reads — while the fields that are not credentials (username, key name)
//! keep echoing what they hold. The masks come from `text_input`'s masked mode; this
//! file proves the tab puts that mode on the right fields and nowhere else.

use api_model::{ApiKeyLocation, Auth, HttpRequest};
use gpui::{Entity, TestAppContext, VisualTestContext};
use text_input::TextInput;
use ui::test_support::{draw_window, init_test};

use super::AuthTab;
use crate::RequestEditor;

/// Obviously fake fixture credentials, so secret scanners and reviewers do not trip.
const FAKE_USERNAME: &str = "user-REDACTED-example";
const FAKE_PASSWORD: &str = "password-REDACTED-example";
const FAKE_TOKEN: &str = "token-REDACTED-example";
const FAKE_KEY_NAME: &str = "X-API-Key-Example";
const FAKE_KEY_VALUE: &str = "key-REDACTED-example";

/// A request carrying `auth`, its Auth tab already built.
fn open_auth_tab(cx: &mut TestAppContext, auth: Auth) -> (Entity<AuthTab>, &mut VisualTestContext) {
    let mut request: HttpRequest = crate::new_request::fresh_request();
    request.auth = auth;
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (cx.read(|cx| editor.read(cx).auth_tab().clone()), cx)
}

/// What one field reports, holds and shows: `(masked, value, display)`.
fn field_state(input: &Entity<TextInput>, cx: &VisualTestContext) -> (bool, String, String) {
    cx.read(|cx| {
        input.read_with(cx, |input, _| {
            (input.is_masked(), input.text(), input.display_text())
        })
    })
}

/// The tab's five stored fields, in [`super::AuthField`] order.
fn fields_of(tab: &Entity<AuthTab>, cx: &VisualTestContext) -> [Entity<TextInput>; 5] {
    cx.read(|cx| {
        let tab = tab.read(cx);
        [
            tab.basic_username_input.clone(),
            tab.basic_password_input.clone(),
            tab.bearer_token_input.clone(),
            tab.api_key_name_input.clone(),
            tab.api_key_value_input.clone(),
        ]
    })
}

#[gpui::test]
fn test_the_auth_tab_masks_its_credential_fields_and_only_those(cx: &mut TestAppContext) {
    let (tab, cx) = open_auth_tab(
        cx,
        Auth::Basic {
            username: Some(FAKE_USERNAME.to_string()),
            password: Some(FAKE_PASSWORD.to_string()),
            extra: Default::default(),
        },
    );
    let fields = fields_of(&tab, cx);

    let (masked, value, display) = field_state(&fields[1], cx);
    assert!(
        masked,
        "the Basic password field is masked (S06: auth \"without exposing credentials\")"
    );
    assert_eq!(
        value, FAKE_PASSWORD,
        "masking is display-deep: the field still holds and reports the password"
    );
    assert!(
        !display.contains(FAKE_PASSWORD),
        "the password never reaches what the field shows"
    );
    assert_eq!(
        display.chars().count(),
        FAKE_PASSWORD.chars().count(),
        "one mask per character of the password"
    );

    let (masked, value, display) = field_state(&fields[0], cx);
    assert!(
        !masked,
        "the username is not a credential: it keeps echoing what it holds"
    );
    assert_eq!(display, value, "an unmasked field shows its value");

    assert!(
        field_state(&fields[2], cx).0,
        "the Bearer token field is masked"
    );
    assert!(
        field_state(&fields[4], cx).0,
        "the API key value field is masked"
    );
    assert!(
        !field_state(&fields[3], cx).0,
        "the API key's name is not a credential either (S03 keeps secret reveal separate)"
    );
}

#[gpui::test]
fn test_a_masked_credential_never_shows_but_still_reaches_the_request_draft(
    cx: &mut TestAppContext,
) {
    let (tab, cx) = open_auth_tab(
        cx,
        Auth::Bearer {
            token: Some(FAKE_TOKEN.to_string()),
            extra: Default::default(),
        },
    );
    let token_input = fields_of(&tab, cx)[2].clone();
    let (_, value, display) = field_state(&token_input, cx);

    assert!(
        !display.contains(FAKE_TOKEN),
        "the token never reaches what the field shows"
    );
    assert_eq!(
        display.chars().count(),
        FAKE_TOKEN.chars().count(),
        "one mask per character of the token"
    );
    assert_eq!(
        value, FAKE_TOKEN,
        "masking is display-deep: the field still holds the token"
    );
    assert_eq!(
        cx.read(|cx| tab.read(cx).auth()),
        Auth::Bearer {
            token: Some(FAKE_TOKEN.to_string()),
            extra: Default::default(),
        },
        "and the draft still sends it"
    );
}

#[gpui::test]
fn test_the_api_key_value_masks_while_its_name_stays_readable(cx: &mut TestAppContext) {
    let (tab, cx) = open_auth_tab(
        cx,
        Auth::ApiKey {
            key: Some(FAKE_KEY_NAME.to_string()),
            value: Some(FAKE_KEY_VALUE.to_string()),
            location: Some(ApiKeyLocation::Header),
            extra: Default::default(),
        },
    );
    let fields = fields_of(&tab, cx);

    let (_, _, display) = field_state(&fields[4], cx);
    assert!(
        !display.contains(FAKE_KEY_VALUE),
        "the API key value never reaches what the field shows"
    );
    assert_eq!(
        display.chars().count(),
        FAKE_KEY_VALUE.chars().count(),
        "one mask per character of the API key value"
    );

    let (masked, value, display) = field_state(&fields[3], cx);
    assert!(!masked, "the API key's name is not a credential");
    assert_eq!(
        display, FAKE_KEY_NAME,
        "the name the user chose stays readable"
    );
    assert_eq!(value, FAKE_KEY_NAME);
}
