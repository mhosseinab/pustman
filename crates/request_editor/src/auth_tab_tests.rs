//! The Auth tab driven the way a user drives it: clicking the tab in the strip,
//! picking an auth type and typing into its fields — asserting the inherited-source
//! label, the REQ-VAR-SHARE-03 secret hint and what lands in the request draft behind
//! the tab (S28b).

use std::collections::BTreeMap;

use api_model::{Auth, HttpRequest};
use gpui::{
    Entity, Focusable, Modifiers, OperatingSystem, SharedString, TestAppContext, VisualTestContext,
};
use ui::test_support::{draw_window, init_test};

use super::{AuthTab, AuthType};
use crate::RequestEditor;

/// The literal a secret field must never hold for long — an obviously fake fixture,
/// so secret scanners and reviewers do not trip on it either.
const LITERAL_TOKEN: &str = "token-REDACTED-example";

/// A draft that inherits its auth — the inheritance label resolves against parents.
fn inheriting_request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.auth = Auth::Inherit;
    request
}

/// A request tab on the app's real init order.
fn open_editor(
    cx: &mut TestAppContext,
    request: HttpRequest,
) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (editor, cx)
}

/// Open the Auth tab the way a user does: click it in the request tab strip.
fn open_auth_tab(cx: &mut VisualTestContext) {
    let tab = cx
        .debug_bounds("request-tab-auth")
        .expect("the Auth tab is in the request tab strip");
    cx.simulate_click(tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

/// The Auth tab entity of `editor`.
fn auth_tab_of(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Entity<AuthTab> {
    cx.read(|cx| editor.read(cx).auth_tab().clone())
}

/// The request draft's auth — what every tab edit must reach.
fn draft_auth(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Auth {
    cx.read(|cx| editor.read(cx).draft().auth.clone())
}

/// Type `text` into the Bearer token field, the way a user does.
fn type_token(auth_tab: &Entity<AuthTab>, cx: &mut VisualTestContext, text: &str) {
    cx.update(|window, cx| {
        let focus_handle = auth_tab
            .read(cx)
            .bearer_token_input()
            .read(cx)
            .focus_handle(cx);
        window.focus(&focus_handle, cx);
    });
    draw_window(cx);
    cx.simulate_input(text);
    cx.run_until_parked();
    draw_window(cx);
}

/// Replace the field's whole text: select all, then type (typing over a selection
/// replaces it — `text_input::insert_text`).
fn replace_token(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_keystrokes(select_all_keys(cx));
    cx.simulate_input(text);
    cx.run_until_parked();
    draw_window(cx);
}

/// The select-all keystroke on this platform, exactly as `text_input::init` binds it.
fn select_all_keys(cx: &VisualTestContext) -> &'static str {
    let os = cx.read(|cx| cx.platform_info().os);
    if os == OperatingSystem::Mac {
        "cmd-a"
    } else {
        "ctrl-a"
    }
}

#[gpui::test]
fn auth_tab_shows_inherited_source(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, inheriting_request());
    open_auth_tab(cx);
    let auth_tab = auth_tab_of(&editor, cx);

    // The chain a workspace supplies: nearest folder first, collection last. Both
    // levels pass, and the collection is the one that decides.
    auth_tab.update(cx, |tab, cx| {
        tab.set_parents(
            vec![
                (SharedString::new_static("Users"), Auth::Inherit),
                (
                    SharedString::new_static("My API"),
                    Auth::Bearer {
                        token: Some(String::from("{{token}}")),
                        extra: BTreeMap::new(),
                    },
                ),
            ],
            cx,
        );
    });
    draw_window(cx);

    let label = cx.read(|cx| auth_tab.read(cx).inherited_source_label());
    assert_eq!(
        label.to_string(),
        "Inherited from My API",
        "the tab names the level the inherited auth comes from: {label}"
    );

    // Picking a type is the request override (REQ-MODEL-01): the draft carries it.
    auth_tab.update(cx, |tab, cx| tab.set_auth_type(AuthType::Basic, cx));
    cx.run_until_parked();
    let auth = draft_auth(&editor, cx);
    assert_eq!(
        auth,
        Auth::Basic {
            username: None,
            password: None,
            extra: BTreeMap::new(),
        },
        "picking Basic writes the override into the request draft: {auth:?}"
    );
}

#[gpui::test]
fn literal_token_shows_secret_hint(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, crate::new_request::fresh_request());
    open_auth_tab(cx);
    let auth_tab = auth_tab_of(&editor, cx);
    auth_tab.update(cx, |tab, cx| tab.set_auth_type(AuthType::Bearer, cx));
    draw_window(cx);

    // A referenced token is not a literal: no nudge.
    type_token(&auth_tab, cx, "{{token}}");
    assert!(
        !cx.read(|cx| auth_tab.read(cx).shows_secret_hint()),
        "typing a variable reference shows no secret hint"
    );

    // A typed-in token is: the hint comes up without blocking the edit — the draft
    // carries the literal exactly as typed.
    replace_token(cx, LITERAL_TOKEN);
    assert!(
        cx.read(|cx| auth_tab.read(cx).shows_secret_hint()),
        "typing a literal token shows the secret hint"
    );
    let auth = draft_auth(&editor, cx);
    assert_eq!(
        auth,
        Auth::Bearer {
            token: Some(String::from(LITERAL_TOKEN)),
            extra: BTreeMap::new(),
        },
        "the nudge never blocks the edit: {auth:?}"
    );

    // And the hint tracks the field: a reference again, no hint again.
    replace_token(cx, "{{token}}");
    assert!(
        !cx.read(|cx| auth_tab.read(cx).shows_secret_hint()),
        "the secret hint goes down once the token is a variable reference again"
    );
}
