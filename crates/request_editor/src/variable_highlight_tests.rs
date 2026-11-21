//! `{{variable}}` highlighting across the request fields (REQ-VAR-03): a field's
//! placeholders carry the resolved, unresolved or secret style on exactly their byte
//! ranges, and the styles follow the scopes — re-highlighted when `set_scopes` changes
//! a variable's state and when a field is edited.

use std::collections::BTreeMap;

use api_model::{Auth, Body, HttpRequest, KeyValue, RawLanguage, Variable, VariableKind};
use gpui::{Entity, HighlightStyle, TestAppContext, VisualTestContext};
use text_input::Highlight;
use ui::prelude::ActiveTheme;
use ui::test_support::init_test;
use variables::{ScopeKind, ScopeStack};

use crate::RequestEditor;
use crate::key_value_table::RowField;

/// Scopes holding one plain variable and one secret. `missing` is bound by nothing,
/// so the URL below exercises all three states at once.
fn scopes() -> ScopeStack {
    let mut scopes = ScopeStack::new();
    scopes.push(
        ScopeKind::Environment,
        vec![
            variable("host", Some("example.test"), VariableKind::Default),
            variable("token", None, VariableKind::Secret),
        ],
    );
    scopes
}

/// The same scopes with `host` gone — its state changes from resolved to unresolved.
fn scopes_without_host() -> ScopeStack {
    let mut scopes = ScopeStack::new();
    scopes.push(
        ScopeKind::Environment,
        vec![variable("token", None, VariableKind::Secret)],
    );
    scopes
}

/// One enabled variable as a scope records it. `value` is a fixture placeholder like
/// `example.test` — never a real secret.
fn variable(key: &str, value: Option<&str>, kind: VariableKind) -> Variable {
    Variable {
        key: key.to_string(),
        value: value.map(str::to_string),
        kind,
        enabled: true,
        description: None,
        extra: BTreeMap::new(),
    }
}

/// A request whose URL, header value, raw body and bearer token each carry
/// `{{variables}}` — one field of every kind the highlighter hooks into.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.url = String::from("{{host}}/x/{{missing}}?k={{token}}");
    request.headers = vec![KeyValue {
        key: String::from("Authorization"),
        value: Some(String::from("Bearer {{token}}")),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }];
    request.body = Body::Raw {
        language: RawLanguage::Json,
        text: String::from("{\"url\":\"{{host}}\"}"),
        extra: BTreeMap::new(),
    };
    request.auth = Auth::Bearer {
        token: Some(String::from("{{token}}")),
        extra: BTreeMap::new(),
    };
    request
}

/// The three styles the fields paint with, read from the theme the code reads.
fn styles(cx: &VisualTestContext) -> (HighlightStyle, HighlightStyle, HighlightStyle) {
    cx.read(|cx| {
        let colors = cx.theme().colors();
        (
            HighlightStyle {
                color: Some(colors.variable_resolved),
                ..HighlightStyle::default()
            },
            HighlightStyle {
                color: Some(colors.variable_unresolved),
                ..HighlightStyle::default()
            },
            HighlightStyle {
                color: Some(colors.text_muted),
                background_color: Some(colors.element_background),
                ..HighlightStyle::default()
            },
        )
    })
}

/// The spans on the URL field.
fn url_highlights(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Vec<Highlight> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor
                .url_bar()
                .read(cx)
                .url_input()
                .read(cx)
                .highlights()
                .to_vec()
        })
    })
}

/// The spans on the first header row's value cell — the key/value table's inputs
/// reached through [`RowField`] (REQ-VAR-03 covers header values).
fn header_value_highlights(
    editor: &Entity<RequestEditor>,
    cx: &VisualTestContext,
) -> Vec<Highlight> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            let table = editor.headers().read(cx).table().clone();
            let (_, value_input) = table
                .read(cx)
                .cell_inputs()
                .into_iter()
                .find(|(field, _)| *field == RowField::Value)
                .expect("the header row has a value cell");
            value_input.read(cx).highlights().to_vec()
        })
    })
}

/// The spans on the raw body's text area.
fn body_highlights(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Vec<Highlight> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor
                .body_tab()
                .read(cx)
                .raw_text_area()
                .read(cx)
                .highlights()
                .to_vec()
        })
    })
}

/// The spans on the bearer token field.
fn auth_highlights(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Vec<Highlight> {
    cx.read(|cx| {
        editor.read_with(cx, |editor, cx| {
            editor
                .auth_tab()
                .read(cx)
                .bearer_token_input()
                .read(cx)
                .highlights()
                .to_vec()
        })
    })
}

#[gpui::test]
fn resolved_unresolved_secret_spans_highlighted(cx: &mut TestAppContext) {
    cx.update(init_test);
    cx.update(|cx| {
        text_input::init(cx);
    });
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request(), window, cx));
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| editor.set_scopes(scopes(), cx));
    });

    let (resolved, unresolved, secret) = styles(cx);
    assert_eq!(
        url_highlights(&editor, cx),
        vec![(0..8, resolved), (11..22, unresolved), (25..34, secret)],
        "the URL's {{host}}, {{missing}} and {{token}} carry their own style on exactly their range"
    );
    assert_eq!(
        header_value_highlights(&editor, cx),
        vec![(7..16, secret)],
        "a header value's {{token}} is masked like the URL's"
    );
    assert_eq!(
        body_highlights(&editor, cx),
        vec![(8..16, resolved)],
        "the raw body's {{host}} resolves"
    );
    assert_eq!(
        auth_highlights(&editor, cx),
        vec![(0..9, secret)],
        "the bearer token's {{token}} is masked"
    );

    // `set_scopes` re-highlights every field when a variable's state changes.
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_scopes(scopes_without_host(), cx)
        });
    });
    assert_eq!(
        url_highlights(&editor, cx),
        vec![(0..8, unresolved), (11..22, unresolved), (25..34, secret)],
        "{{host}} turns unresolved once no scope holds it"
    );
    assert_eq!(
        body_highlights(&editor, cx),
        vec![(8..16, unresolved)],
        "the body's {{host}} follows the scopes too"
    );
    assert_eq!(
        header_value_highlights(&editor, cx),
        vec![(7..16, secret)],
        "the masked header value does not change with the scopes"
    );
    assert_eq!(
        auth_highlights(&editor, cx),
        vec![(0..9, secret)],
        "the masked bearer token does not change with the scopes"
    );

    // An edit re-highlights the field it landed in (TextInputEvent::Changed).
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor
                .url_bar()
                .update(cx, |bar, cx| bar.set_url("{{token}}", cx));
        });
    });
    cx.run_until_parked();
    assert_eq!(
        url_highlights(&editor, cx),
        vec![(0..9, secret)],
        "editing the URL re-highlights it against the current scopes"
    );
}
