//! The `{{var}}` popover (S48u6, mockup section 7), driven the user's way: the pointer
//! (or the caret) lands on a placeholder, the value is edited and Enter pressed, a
//! button clicked. The writer is the app wire's boundary ([`VariableWrites`]); here it
//! records what it is asked to land — `environment_picker`'s own tests prove the write
//! reaches the file. Every text assertion reads [`VariableHover::rendered_texts`], the
//! same model the popover draws.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use api_model::{HttpRequest, MASK, Variable, VariableKind};
use gpui::{
    Entity, Focusable, Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px,
};
use pustman_actions::environment::EditSecret;
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use ui::test_support::{draw_window, init_test, press_key};
use variables::{ScopeKind, ScopeStack};

use super::{
    PopoverAction, SecretSources, VARIABLE_HOVER_TARGET_SELECTOR, VariableEdit, VariableWrites,
    popover_button_selector,
};
use crate::RequestEditor;

const FAKE_SECRET: &str = "token-REDACTED-example";
const BASE_URL: &str = "https://api.dev.example.test/v1";

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

fn scopes() -> ScopeStack {
    let mut scopes = ScopeStack::new();
    scopes.push(
        ScopeKind::Environment,
        vec![
            variable("base_url", Some(BASE_URL), VariableKind::Default),
            variable("token", None, VariableKind::Secret),
        ],
    );
    scopes
}

fn sources() -> SecretSources {
    let store = MemorySecretStore::new();
    let key = SecretKey::new("workspace-a", "environment-a", "token")
        .expect("the fixture ids are valid keychain ids");
    store
        .set(&key, &SecretValue::new(FAKE_SECRET))
        .expect("the fixture secret stores");
    SecretSources {
        store: Arc::new(store),
        key_of: Arc::new(|kind, name| {
            (kind == ScopeKind::Environment)
                .then(|| SecretKey::new("workspace-a", "environment-a", name).ok())
                .flatten()
        }),
    }
}

type Recorded = Arc<Mutex<Vec<VariableEdit>>>;

fn writes(recorded: &Recorded) -> VariableWrites {
    let recorded = Arc::clone(recorded);
    VariableWrites {
        scope_names: vec![
            (ScopeKind::Environment, String::from("Development")),
            (ScopeKind::Global, String::from("Globals")),
        ],
        write: Some(Arc::new(move |edit, _window, _cx| {
            recorded.lock().expect("the record locks").push(edit)
        })),
    }
}

fn open_editor<'a>(
    url: &str,
    recorded: &Recorded,
    cx: &'a mut TestAppContext,
) -> (Entity<RequestEditor>, &'a mut VisualTestContext) {
    cx.update(init_test);
    cx.update(text_input::init);
    let mut request: HttpRequest = crate::new_request::fresh_request();
    request.url = url.to_string();
    let (editor, cx) = cx.add_window_view(|window, cx| RequestEditor::new(request, window, cx));
    cx.update(|window, _cx| window.activate_window());
    let writes = writes(recorded);
    cx.update(|_window, cx| {
        editor.update(cx, |editor, cx| {
            editor.set_scopes(scopes(), cx);
            editor.set_secret_sources(sources(), cx);
            editor.set_variable_writes(writes, cx);
        });
    });
    draw_window(cx);
    (editor, cx)
}

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
    draw_window(cx);
}

fn hover_of(
    editor: &Entity<RequestEditor>,
    cx: &VisualTestContext,
) -> Entity<super::VariableHover> {
    cx.read(|cx| editor.read(cx).url_bar().read(cx).hover().clone())
}

fn texts(editor: &Entity<RequestEditor>, cx: &VisualTestContext) -> Vec<String> {
    let hover = hover_of(editor, cx);
    cx.read(|cx| hover.read(cx).rendered_texts(cx))
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} renders"));
    cx.simulate_click(bounds.center(), Modifiers::none());
    cx.run_until_parked();
    draw_window(cx);
}

#[gpui::test]
fn test_a_plain_value_is_edited_in_the_popover_and_saved_to_its_scope(cx: &mut TestAppContext) {
    let recorded: Recorded = Arc::default();
    let (editor, cx) = open_editor("{{base_url}}/orders", &recorded, cx);
    hover_url(cx);

    let shown = texts(&editor, cx);
    assert!(
        shown.contains(&String::from("E")),
        "the scope badge: {shown:?}"
    );
    assert!(
        shown.contains(&String::from("Environment · Development")),
        "the scope line names the environment: {shown:?}"
    );
    assert!(
        shown.contains(&String::from(BASE_URL)),
        "the field holds the value: {shown:?}"
    );

    let field = hover_of(&editor, cx).read_with(cx, |hover, _| hover.value_input().clone());
    cx.update(|window, cx| {
        field.update(cx, |field, cx| {
            field.set_text("https://api.staging.example.test/v1", cx)
        });
        let handle = field.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();

    assert_eq!(
        recorded.lock().expect("the record locks").as_slice(),
        &[VariableEdit {
            scope: ScopeKind::Environment,
            name: String::from("base_url"),
            value: String::from("https://api.staging.example.test/v1"),
            local: false,
        }],
        "Enter lands the edit in the scope base_url resolves from"
    );
}

#[gpui::test]
fn test_a_secret_shows_the_mask_only_and_its_value_is_in_no_rendered_text(cx: &mut TestAppContext) {
    let recorded: Recorded = Arc::default();
    let (editor, cx) = open_editor("{{token}}", &recorded, cx);
    let edit_secret: Rc<RefCell<Vec<String>>> = Rc::default();
    cx.update(|_window, cx| {
        let edit_secret = Rc::clone(&edit_secret);
        cx.on_action(move |action: &EditSecret, _cx| {
            edit_secret.borrow_mut().push(action.name.clone())
        });
    });
    hover_url(cx);

    let shown = texts(&editor, cx);
    assert!(shown.contains(&String::from(MASK)), "the mask: {shown:?}");
    assert!(
        shown.contains(&String::from("Edit secret…")),
        "the secret is edited where it lives: {shown:?}"
    );
    assert!(
        shown.iter().all(|text| !text.contains(FAKE_SECRET)),
        "the secret value is in no rendered text: {shown:?}"
    );

    click(cx, popover_button_selector(PopoverAction::EditSecret));
    assert_eq!(
        edit_secret.borrow().as_slice(),
        &[String::from("token")],
        "Edit secret… opens the editor on the secret, by name"
    );
    assert!(
        recorded.lock().expect("the record locks").is_empty(),
        "nothing is written for a secret"
    );
}

#[gpui::test]
fn test_an_undefined_variable_is_added_to_the_active_environment(cx: &mut TestAppContext) {
    let recorded: Recorded = Arc::default();
    let (editor, cx) = open_editor("{{page_size}}", &recorded, cx);
    hover_url(cx);

    let shown = texts(&editor, cx);
    assert!(
        shown.contains(&String::from("page_size is not defined")),
        "{shown:?}"
    );
    assert!(
        shown.contains(&String::from("Add to Development")),
        "{shown:?}"
    );
    assert!(shown.contains(&String::from("Add to Globals")), "{shown:?}");

    click(
        cx,
        popover_button_selector(PopoverAction::AddTo(ScopeKind::Environment)),
    );

    assert_eq!(
        recorded.lock().expect("the record locks").as_slice(),
        &[VariableEdit {
            scope: ScopeKind::Environment,
            name: String::from("page_size"),
            value: String::new(),
            local: false,
        }],
        "Add to Development writes the variable into the environment"
    );
}

#[gpui::test]
fn test_the_caret_inside_a_placeholder_opens_the_popover(cx: &mut TestAppContext) {
    let recorded: Recorded = Arc::default();
    let (editor, cx) = open_editor("{{base_url}}/orders", &recorded, cx);
    let input = cx.read(|cx| editor.read(cx).url_bar().read(cx).url_input().clone());
    cx.update(|window, cx| {
        let handle = input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });
    draw_window(cx);
    press_key(cx, "home");
    for _ in 0..3 {
        press_key(cx, "right");
    }
    cx.run_until_parked();
    draw_window(cx);

    assert!(
        texts(&editor, cx).contains(&String::from("base_url")),
        "the caret inside {{{{base_url}}}} opens its popover"
    );

    press_key(cx, "end");
    cx.run_until_parked();
    assert!(
        texts(&editor, cx).is_empty(),
        "leaving the placeholder closes it"
    );
}
