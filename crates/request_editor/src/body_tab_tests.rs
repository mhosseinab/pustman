//! The Body tab driven the way a user drives it: clicking the tab in the strip,
//! switching body modes, typing into the editors and answering the file dialog —
//! asserting what lands in the request draft behind the tab (S28a).

use std::collections::BTreeMap;
use std::path::PathBuf;

use api_model::{Body, HttpRequest, KeyValue, Method, RawLanguage};
use gpui::{Entity, Focusable, Modifiers, TestAppContext, VisualTestContext};
use ui::test_support::{draw_window, init_test};

use super::{BodyMode, BodyTab, FileTarget, FormFieldKind, stored_file_path};
use crate::RequestEditor;
use crate::key_value_table::RowField;

/// A draft POST — a body belongs on a method that carries one.
fn request() -> HttpRequest {
    let mut request = crate::new_request::fresh_request();
    request.method = Method::Post;
    request
}

/// A row with no description and nothing imported in `extra`.
fn key_value(key: &str, value: Option<&str>) -> KeyValue {
    KeyValue {
        key: String::from(key),
        value: value.map(String::from),
        description: None,
        enabled: true,
        extra: BTreeMap::new(),
    }
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

/// Open the Body tab the way a user does: click it in the request tab strip.
fn open_body_tab(cx: &mut VisualTestContext) {
    let tab = cx
        .debug_bounds("request-tab-body")
        .expect("the Body tab is in the request tab strip");
    cx.simulate_click(tab.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

/// The Body tab entity of `editor`.
fn body_tab_of(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Entity<BodyTab> {
    cx.read(|cx| editor.read(cx).body_tab().clone())
}

/// The request draft's body — what every tab edit must reach.
fn draft_body(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Body {
    cx.read(|cx| editor.read(cx).draft().body.clone())
}

/// Type `text` into the raw body editor, the way a user does.
fn type_raw(body_tab: &Entity<BodyTab>, cx: &mut VisualTestContext, text: &str) {
    cx.update(|window, cx| {
        let focus_handle = body_tab.read(cx).raw_text_area().read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
    });
    draw_window(cx);
    cx.simulate_input(text);
    cx.run_until_parked();
    draw_window(cx);
}

/// Add one urlencoded row and type it in, the way a user does.
fn type_urlencoded_row(
    body_tab: &Entity<BodyTab>,
    cx: &mut VisualTestContext,
    key: &str,
    value: &str,
) {
    let table = cx.read(|cx| body_tab.read(cx).urlencoded_table().clone());
    cx.update(|window, cx| {
        table.update(cx, |table, cx| table.add_row(window, cx));
    });
    draw_window(cx);
    let row_id = cx
        .read(|cx| table.read(cx).row_ids().first().copied())
        .expect("the added row exists");
    for (field, text) in [(RowField::Key, key), (RowField::Value, value)] {
        cx.update(|window, cx| {
            table.update(cx, |table, cx| table.focus_cell(row_id, field, window, cx));
        });
        draw_window(cx);
        cx.simulate_input(text);
        cx.run_until_parked();
        draw_window(cx);
    }
}

#[gpui::test]
fn body_mode_switch_preserves_other_modes_data(cx: &mut TestAppContext) {
    let (editor, cx) = open_editor(cx, request());
    open_body_tab(cx);
    let body_tab = body_tab_of(&editor, cx);

    // Type a raw body.
    body_tab.update(cx, |tab, cx| tab.set_mode(BodyMode::Raw, cx));
    type_raw(&body_tab, cx, "hello body");
    let body = draft_body(&editor, cx);
    assert_eq!(
        body,
        Body::Raw {
            language: RawLanguage::Text,
            text: String::from("hello body"),
            extra: BTreeMap::new(),
        },
        "typing into the raw editor lands in the request draft: {body:?}"
    );

    // Switch to urlencoded and type one row.
    body_tab.update(cx, |tab, cx| tab.set_mode(BodyMode::UrlEncoded, cx));
    type_urlencoded_row(&body_tab, cx, "token", "abc");
    let row = key_value("token", Some("abc"));
    let body = draft_body(&editor, cx);
    assert_eq!(
        body,
        Body::UrlEncoded {
            fields: vec![row.clone()],
            extra: BTreeMap::new(),
        },
        "the urlencoded row lands in the request draft: {body:?}"
    );

    // Switch back to raw: the text survived the detour and is again the draft's body.
    body_tab.update(cx, |tab, cx| tab.set_mode(BodyMode::Raw, cx));
    let raw_text = cx.read(|cx| body_tab.read(cx).raw_text_area().read(cx).text());
    assert_eq!(
        raw_text, "hello body",
        "the raw text survived the mode switches"
    );
    let body = draft_body(&editor, cx);
    assert_eq!(
        body,
        Body::Raw {
            language: RawLanguage::Text,
            text: String::from("hello body"),
            extra: BTreeMap::new(),
        },
        "the draft holds the raw body again: {body:?}"
    );

    // And the urlencoded row survived its detour just as well.
    body_tab.update(cx, |tab, cx| tab.set_mode(BodyMode::UrlEncoded, cx));
    let body = draft_body(&editor, cx);
    assert_eq!(
        body,
        Body::UrlEncoded {
            fields: vec![row],
            extra: BTreeMap::new(),
        },
        "the urlencoded row survived the mode switches: {body:?}"
    );
}

#[gpui::test]
fn file_path_saved_relative_to_workspace(cx: &mut TestAppContext) {
    // The storage rule (REQ-HTTP-05) as a pure function: inside the root is relative
    // with `/` separators; outside — or with no root at all — is kept as picked.
    let root = PathBuf::from("/work/pustman");
    let inside = root.join("sub").join("file.bin");
    let outside = PathBuf::from("/elsewhere/file.bin");
    assert_eq!(stored_file_path(Some(&root), &inside), "sub/file.bin");
    assert_eq!(
        stored_file_path(Some(&root), &outside),
        outside.to_string_lossy().as_ref(),
        "a file outside the workspace is stored absolute"
    );
    assert_eq!(
        stored_file_path(None, &inside),
        inside.to_string_lossy().as_ref(),
        "with no workspace root every file is stored absolute"
    );

    // Through the tab: a form-data file field picked inside the workspace stores the
    // relative path in the draft.
    let (editor, cx) = open_editor(cx, request());
    open_body_tab(cx);
    let body_tab = body_tab_of(&editor, cx);
    body_tab.update(cx, |tab, cx| {
        tab.set_workspace_root(Some(root.clone()));
        tab.set_mode(BodyMode::FormData, cx);
    });
    draw_window(cx);
    let row_id = cx.update(|window, cx| {
        body_tab.update(cx, |tab, cx| {
            tab.add_form_field(FormFieldKind::File, window, cx)
        })
    });
    draw_window(cx);
    cx.update(|window, cx| {
        body_tab.update(cx, |tab, cx| {
            tab.choose_file(FileTarget::FormField(row_id), window, cx)
        });
    });
    cx.simulate_path_prompt_response({
        let inside = inside.clone();
        move |_options| Some(vec![inside.clone()])
    });
    cx.run_until_parked();
    draw_window(cx);

    let body = draft_body(&editor, cx);
    let Body::FormData { fields, .. } = &body else {
        panic!("the draft holds the form body: {body:?}");
    };
    assert_eq!(
        fields.first().and_then(|field| field.file.as_deref()),
        Some("sub/file.bin"),
        "a file inside the workspace is stored relative to its root: {body:?}"
    );

    // And the binary body's file picked outside the workspace stays absolute.
    body_tab.update(cx, |tab, cx| tab.set_mode(BodyMode::Binary, cx));
    cx.update(|window, cx| {
        body_tab.update(cx, |tab, cx| {
            tab.choose_file(FileTarget::Binary, window, cx)
        });
    });
    cx.simulate_path_prompt_response({
        let outside = outside.clone();
        move |_options| Some(vec![outside.clone()])
    });
    cx.run_until_parked();

    let body = draft_body(&editor, cx);
    assert_eq!(
        body,
        Body::Binary {
            file: outside.to_string_lossy().into_owned(),
            extra: BTreeMap::new(),
        },
        "a file outside the workspace is stored as picked: {body:?}"
    );
}
