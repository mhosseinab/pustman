//! The variable line under the active table names each `{{variable}}` the request uses:
//! a resolved one with its value and source, a secret only as the mask (never its
//! value), and an unresolved one with "Set it…", which dispatches the existing fix —
//! `environment::EditSecret` for a secret, the variables editor's open action otherwise.
//! Every value here is fake.

use std::cell::RefCell;
use std::rc::Rc;

use api_model::{HttpRequest, MASK, Variable, VariableKind};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use pustman_actions::environment::EditSecret;
use ui::test_support::{draw_window, init_test};
use variables::{ScopeKind, ScopeStack};

use super::{EntryState, VARIABLE_LINE_SELECTOR, VariableFix};
use crate::RequestEditor;

// The variables editor's open action is `environment_picker`'s; this test binary does
// not link that crate, so it registers an action under the same name to observe the
// by-name dispatch.
gpui::actions!(env_editor, [Open]);

/// Obviously fake: a secret value that must never reach the line.
const FAKE_SECRET: &str = "fake-secret-REDACTED-example";

fn variable(key: &str, value: Option<&str>, kind: VariableKind) -> Variable {
    Variable {
        key: key.to_string(),
        value: value.map(str::to_string),
        kind,
        enabled: true,
        description: None,
        extra: Default::default(),
    }
}

fn scopes() -> ScopeStack {
    let mut scopes = ScopeStack::new();
    scopes.push(
        ScopeKind::Environment,
        vec![
            variable(
                "base_url",
                Some("https://api.dev.example.test"),
                VariableKind::Default,
            ),
            variable("api_key", None, VariableKind::Secret),
            variable("token", None, VariableKind::Secret),
        ],
    );
    scopes.set_local_value("api_key", FAKE_SECRET);
    scopes
}

fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("{{base_url}}/orders?limit={{page_size}}&key={{api_key}}");
    request.auth = api_model::Auth::Bearer {
        token: Some(String::from("{{token}}")),
        extra: Default::default(),
    };
    request
}

fn open(cx: &mut TestAppContext) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        crate::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request(), window, cx));
    cx.update(|_, cx| editor.update(cx, |editor, cx| editor.set_scopes(scopes(), cx)));
    draw_window(cx);
    (editor, cx)
}

fn state_of(editor: &Entity<RequestEditor>, cx: &mut VisualTestContext, name: &str) -> EntryState {
    editor.read_with(cx, |editor, _| {
        editor
            .variable_line()
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.state.clone())
            .unwrap_or_else(|| panic!("{name} is on the line"))
    })
}

#[gpui::test]
fn test_the_variable_line_shows_source_and_masks_secrets(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx);
    assert!(cx.debug_bounds(VARIABLE_LINE_SELECTOR).is_some());

    assert_eq!(
        state_of(&editor, cx, "base_url"),
        EntryState::Resolved {
            shown: String::from("https://api.dev.example.test"),
            source: String::from("Source: Environment"),
        }
    );
    assert_eq!(
        state_of(&editor, cx, "api_key"),
        EntryState::Resolved {
            shown: MASK.to_string(),
            source: String::from("Source: Environment"),
        },
        "a secret with a value shows only the mask"
    );
    let names: Vec<String> = editor.read_with(cx, |editor, _| {
        editor
            .variable_line()
            .iter()
            .map(|entry| entry.name.clone())
            .collect()
    });
    assert_eq!(names, ["base_url", "page_size", "api_key", "token"]);
    let leaked = editor.read_with(cx, |editor, _| {
        editor
            .variable_line()
            .iter()
            .any(|entry| match &entry.state {
                EntryState::Resolved { shown, source } => {
                    shown.contains(FAKE_SECRET) || source.contains(FAKE_SECRET)
                }
                EntryState::Unresolved { message, .. } => message.contains(FAKE_SECRET),
            })
    });
    assert!(!leaked, "the secret's value never reaches the line");
}

#[gpui::test]
fn test_an_unresolved_secret_set_it_dispatches_edit_secret(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx);
    assert!(matches!(
        state_of(&editor, cx, "token"),
        EntryState::Unresolved { fix: VariableFix::EditSecret(ref name), .. } if name == "token"
    ));
    let seen: Rc<RefCell<Vec<String>>> = Rc::default();
    cx.update(|_, cx| {
        let seen = seen.clone();
        cx.on_action(move |action: &EditSecret, _cx| seen.borrow_mut().push(action.name.clone()));
    });

    let button = cx
        .debug_bounds("variable-fix-token")
        .expect("the unresolved secret offers Set it…");
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();

    assert_eq!(
        *seen.borrow(),
        ["token"],
        "Set it… dispatched EditSecret for the name"
    );
}

#[gpui::test]
fn test_an_undefined_variable_set_it_opens_the_variables_editor(cx: &mut TestAppContext) {
    let (editor, cx) = open(cx);
    match state_of(&editor, cx, "page_size") {
        EntryState::Unresolved { message, fix } => {
            assert_eq!(message, "is not defined in the active environment.");
            assert_eq!(fix, VariableFix::OpenVariablesEditor);
        }
        other => panic!("page_size is unresolved, got {other:?}"),
    }
    let opened = Rc::new(RefCell::new(0));
    cx.update(|_, cx| {
        let opened = opened.clone();
        cx.on_action(move |_: &Open, _cx| *opened.borrow_mut() += 1);
    });

    let button = cx
        .debug_bounds("variable-fix-page_size")
        .expect("the undefined name offers Set it…");
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();

    assert_eq!(*opened.borrow(), 1, "Set it… dispatched env_editor::Open");
}
