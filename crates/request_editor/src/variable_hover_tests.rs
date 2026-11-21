//! `{{variable}}` hover over the URL field (REQ-VAR-03), driven the way a user drives
//! it: the pointer moves onto the highlighted `{{token}}` placeholder, and the reveal
//! toggle is turned with a real key press on its focus handle (Enter on a focused
//! control is one click). A `secrets::MemorySecretStore` stands in for the OS keychain
//! under the same `SecretStore` contract. Every assertion is on the strings the hover
//! displays (via [`super::VariableHover::hovered`], exactly what `render` draws), so a
//! hover that masks nothing, leaks the value into any display string before a reveal,
//! or keeps showing it after hiding fails them.

use std::collections::BTreeMap;
use std::sync::Arc;

use api_model::{HttpRequest, MASK, Variable, VariableKind};
use gpui::{Entity, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use ui::test_support::{draw_window, init_test, press_key};
use variables::{ScopeKind, ScopeStack};

use super::{HoverDisplay, SecretSources, VARIABLE_HOVER_TARGET_SELECTOR};
use crate::RequestEditor;

/// The workspace every fixture's keys and secrets are scoped to.
const WORKSPACE_ID: &str = "workspace-a";

/// The scope id the fixture's secret lives under (an environment's stable id in the
/// real wiring).
const SCOPE_ID: &str = "environment-a";

/// The fixture secret — obviously fake (`.agents/rules/tests.md`): a realistic-looking
/// token trips secret scanners and reviewers.
const FAKE_SECRET: &str = "token-REDACTED-example";

/// The scopes in effect: one environment holding the secret `token` — nothing else
/// resolves, so the URL below is one placeholder and nothing more.
fn scopes() -> ScopeStack {
    let mut scopes = ScopeStack::new();
    scopes.push(
        ScopeKind::Environment,
        vec![Variable {
            key: String::from("token"),
            value: None,
            kind: VariableKind::Secret,
            enabled: true,
            description: None,
            extra: BTreeMap::new(),
        }],
    );
    scopes
}

/// The fixture's key names: an environment-scope secret under the test workspace.
fn fixture_key_of(kind: ScopeKind, name: &str) -> Option<SecretKey> {
    if kind == ScopeKind::Environment {
        SecretKey::new(WORKSPACE_ID, SCOPE_ID, name).ok()
    } else {
        None
    }
}

/// Where the fixture's hovered secret lives: the test's keychain under the same
/// `SecretStore` contract the real store implements, holding the obviously fake value.
fn sources() -> SecretSources {
    let store = MemorySecretStore::new();
    let key = SecretKey::new(WORKSPACE_ID, SCOPE_ID, "token")
        .expect("the fixture's scope ids are valid keychain ids");
    store
        .set(&key, &SecretValue::new(FAKE_SECRET))
        .expect("the fixture secret stores");
    SecretSources {
        store: Arc::new(store),
        key_of: Arc::new(fixture_key_of),
    }
}

/// The draft the hover runs over: a URL that is one placeholder, so the pointer at the
/// field's first character is inside its span.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("{{token}}");
    request
}

/// A request tab on the app's real init order, with the fixture scopes and secrets in
/// effect and the window painted (hitboxes register during a paint).
fn open_editor(cx: &mut TestAppContext) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request(), window, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_scopes(scopes(), cx);
            editor.set_secret_sources(sources(), cx);
        });
    });
    draw_window(cx);
    (editor, cx)
}

/// Hover the URL field the user's way: the pointer moves onto the field's first
/// character, which the URL fixture puts inside `{{token}}`'s span.
fn hover_url(cx: &mut VisualTestContext) {
    let bounds = cx
        .debug_bounds(VARIABLE_HOVER_TARGET_SELECTOR)
        .expect("the URL field's hover wrapper paints");
    cx.simulate_mouse_move(
        point(bounds.left() + px(2.), bounds.top() + px(2.)),
        None::<MouseButton>,
        Modifiers::default(),
    );
    cx.run_until_parked();
}

/// Turn the reveal toggle the keyboard's way: focus the toggle's own handle and press
/// Enter, which GPUI turns into one click on the focused control (the quick-look's
/// reveal shape).
fn press_reveal(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext) {
    let focus_handle = cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor
                .url_bar()
                .read(cx)
                .hover()
                .read(cx)
                .reveal_focus_handle()
        })
    });
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
}

/// The hover as the user sees it, exactly as the popover draws it.
fn hover_display(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Option<HoverDisplay> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor.url_bar().read(cx).hover().read(cx).hovered()
        })
    })
}

/// Every string the hover displays, joined — what "the hover text" means for a leak
/// assertion.
fn display_text(display: &HoverDisplay) -> String {
    [
        display.name.as_str(),
        display.scope.as_str(),
        display.value.as_str(),
    ]
    .join(" ")
}

#[gpui::test]
fn hover_masks_secret_value(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx);
    hover_url(cx);

    let display = hover_display(&editor, cx).expect("hovering {{token}} opens the hover");
    assert_eq!(display.name, "token", "the hover names the variable");
    assert_eq!(
        display.scope, "Environment",
        "the hover names the scope the value came from (REQ-VAR-03)"
    );
    assert_eq!(
        display.value, MASK,
        "a set secret hovers as the mask (REQ-VAR-03)"
    );
    assert!(
        !display_text(&display).contains(FAKE_SECRET),
        "the value appears nowhere in the hover text until reveal"
    );

    press_reveal(&editor, cx);
    let display = hover_display(&editor, cx).expect("the hover stays open through the reveal");
    assert_eq!(
        display.value, FAKE_SECRET,
        "the reveal toggle shows the value it fetched"
    );

    press_reveal(&editor, cx);
    let display = hover_display(&editor, cx).expect("the hover stays open after hiding");
    assert_eq!(display.value, MASK, "toggling off masks the value again");
    assert!(
        !display_text(&display).contains(FAKE_SECRET),
        "hiding the value drops it from the hover text again"
    );
}
