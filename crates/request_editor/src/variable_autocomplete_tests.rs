//! `{{variable}}` autocomplete behind the URL field (REQ-VAR-04), driven the way a user
//! drives it: typing `{{` in the field and stepping the popover with the arrow and
//! Enter keys. Every assertion is an outcome the user can see — the names the popover
//! lists, in order, and the exact template the insert leaves in the field.

use std::collections::BTreeMap;

use api_model::{HttpRequest, Variable, VariableKind};
use gpui::{Entity, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};
use variables::{ScopeKind, ScopeStack};

use crate::RequestEditor;

/// The scopes in effect: `token` twice — the request-local secret shadows the
/// environment's plain one — plus `api_key` and `host` in the environment. The values
/// are obvious fixtures and never read here: the popover lists names only (REQ-VAR-04).
fn scopes() -> ScopeStack {
    let mut scopes = ScopeStack::new();
    scopes.push(
        ScopeKind::Environment,
        vec![
            variable("host", "example.test"),
            variable("token", "token-REDACTED-example"),
            variable("api_key", "key-REDACTED-example"),
        ],
    );
    scopes.push(ScopeKind::RequestLocal, vec![secret("token")]);
    scopes
}

/// One enabled environment variable with an obviously fake value (the test-fixture
/// rule: a secret-looking value must read as fake).
fn variable(key: &str, value: &str) -> Variable {
    Variable {
        key: key.to_string(),
        value: Some(value.to_string()),
        kind: VariableKind::Default,
        enabled: true,
        description: None,
        extra: BTreeMap::new(),
    }
}

/// One enabled secret variable: its name is listed like any other, and its value lives
/// nowhere a fixture can reach.
fn secret(key: &str) -> Variable {
    Variable {
        key: key.to_string(),
        value: None,
        kind: VariableKind::Secret,
        enabled: true,
        description: None,
        extra: BTreeMap::new(),
    }
}

/// The draft the test types into: a plain request whose URL is scheme-only, so the
/// template the insert leaves behind is easy to read.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("https://");
    request
}

/// A request tab on the app's real init order and the real default keymap, with the
/// scopes above in effect and the caret in the URL field — where `{{` is typed.
fn open_editor(cx: &mut TestAppContext) -> (Entity<RequestEditor>, &mut VisualTestContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
        let problems = pustman_actions::load_default_keymap(cx);
        assert!(
            problems.is_empty(),
            "the default keymap must bind every entry: {problems:?}"
        );
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request(), window, cx));
    cx.update(|window, _cx| window.activate_window());
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| editor.set_scopes(scopes(), cx));
    });
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
    });
    draw_window(cx);
    (editor, cx)
}

/// The names the popover lists, in the order it lists them.
fn listed_names(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Vec<String> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor
                .url_bar()
                .read(cx)
                .autocomplete()
                .read(cx)
                .matches()
                .to_vec()
        })
    })
}

/// Whether the popover is showing.
fn popover_open(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> bool {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor.url_bar().read(cx).autocomplete().read(cx).is_open()
        })
    })
}

/// The URL text — the template the insert leaves behind.
fn url_text(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> String {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor.url_bar().read(cx).url_input().read(cx).text()
        })
    })
}

/// The URL field's caret offset.
fn url_caret(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> usize {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor
                .url_bar()
                .read(cx)
                .url_input()
                .read(cx)
                .selected_range()
                .start
        })
    })
}

#[gpui::test]
fn autocomplete_lists_names_after_braces(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx);

    // Typing `{{` opens the popover with every name in scope, each exactly once: the
    // request-local `token` shadows the environment's, and the rest follow the scope
    // order (narrowest first, then by name).
    cx.simulate_input("{{");
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(
        listed_names(&editor, cx),
        vec![
            String::from("token"),
            String::from("api_key"),
            String::from("host"),
        ],
        "typing {{ lists every name in scope once, narrowest scope first then by name"
    );
    assert!(
        cx.debug_bounds("completion-token").is_some(),
        "the popover renders a row for the shadowed name it lists"
    );
    assert!(
        cx.debug_bounds("completion-api_key").is_some()
            && cx.debug_bounds("completion-host").is_some(),
        "the popover renders a row for every name it lists"
    );

    // A typed partial narrows the list to the names that contain it.
    cx.simulate_input("a");
    cx.run_until_parked();
    draw_window(cx);
    assert_eq!(
        listed_names(&editor, cx),
        vec![String::from("api_key")],
        "typing a partial narrows the popover to the matching names"
    );
    assert!(
        cx.debug_bounds("completion-host").is_none(),
        "a name the partial no longer matches leaves the popover"
    );

    // Escape closes the popover and leaves the text alone.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!popover_open(&editor, cx), "Escape closes the popover");
    assert_eq!(
        url_text(&editor, cx),
        "https://{{a",
        "Escape closes the popover without touching the text"
    );
}

#[gpui::test]
fn autocomplete_inserts_selected_name(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx);
    cx.simulate_input("{{");
    cx.run_until_parked();

    // Down moves the selection to the second name; Enter inserts it closed with `}}`.
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    draw_window(cx);

    assert_eq!(
        url_text(&editor, cx),
        "https://{{api_key}}",
        "Down and Enter insert the chosen name with its closing braces"
    );
    assert_eq!(
        url_caret(&editor, cx),
        url_text(&editor, cx).len(),
        "the caret ends after the }} that closes the placeholder"
    );
    assert!(
        !popover_open(&editor, cx),
        "the popover closes once a name is inserted"
    );

    // The same insert with `}}` already behind the caret completes the placeholder
    // instead of writing a second pair of braces — the `text_input::completion`'s
    // insert contract — and lands the caret after `}}` again.
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor
                .url_bar()
                .update(cx, |bar, cx| bar.set_url("https://{{}}", cx));
        });
    });
    cx.simulate_keystrokes("left left");
    cx.simulate_input("o");
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();

    assert_eq!(
        url_text(&editor, cx),
        "https://{{host}}",
        "completing before an existing }} inserts the name without doubling the braces"
    );
    assert_eq!(
        url_caret(&editor, cx),
        url_text(&editor, cx).len(),
        "the caret ends after the }} either way"
    );
}
