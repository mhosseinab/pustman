//! The quick-look is driven the way a user drives it: `environment::QuickLook` through
//! GPUI's action dispatch, and the reveal toggle through a real key press on its focus
//! handle — Enter on a focused control is one click. A `secrets::MemorySecretStore`
//! stands in for the OS keychain under the same `SecretStore` contract. Every assertion
//! is on the strings the dialog displays (via [`super::QuickLook::displayed_rows`],
//! exactly what `render` draws), so a quick-look that masks nothing, leaks a value into
//! the display before a reveal, or labels an unset secret with anything but "unset"
//! fails them.

use std::collections::BTreeMap;
use std::sync::Arc;

use api_model::{Environment, ItemId, MASK, Variable, VariableKind};
use gpui::{Entity, TestAppContext, VisualTestContext};
use pustman_actions::environment::QuickLook as QuickLookAction;
use secrets::{MemorySecretStore, SecretKey, SecretStore, SecretValue};
use ui::test_support::{draw_window, init_test, press_key};
use workspace::Workspace;

use super::{
    DisplayedRow, NO_VALUE_LABEL, QuickLook, QuickLookInputs, UNSET_LABEL, install_inputs,
};

/// The workspace every fixture's keys and secrets are scoped to.
const WORKSPACE_ID: &str = "workspace-a";

/// The fixture secret — obviously fake (`.agents/rules/tests.md`): a realistic-looking
/// token trips secret scanners and reviewers.
const FAKE_SECRET: &str = "token-REDACTED-example";

/// The environment every test lists: one ordinary variable and one secret.
fn staging() -> Environment {
    Environment {
        id: ItemId::new(),
        name: "Staging".to_string(),
        variables: vec![
            Variable {
                key: "base_url".to_string(),
                value: Some("https://staging.example.test".to_string()),
                kind: VariableKind::Default,
                enabled: true,
                description: None,
                extra: BTreeMap::new(),
            },
            Variable {
                key: "api_token".to_string(),
                value: None,
                kind: VariableKind::Secret,
                enabled: true,
                description: None,
                extra: BTreeMap::new(),
            },
        ],
        extra: BTreeMap::new(),
    }
}

/// The keychain key `environment`'s secret `variable_key` lives under — the same scope
/// id convention the dialog uses (module docs).
fn secret_key_for(environment: &Environment, variable_key: &str) -> SecretKey {
    SecretKey::new(WORKSPACE_ID, environment.id.to_string(), variable_key)
        .expect("the fixture's scope ids are valid keychain ids")
}

/// The inputs one open round runs with: `environment`'s variables and `store`'s
/// keychain.
fn inputs(environment: &Environment, store: MemorySecretStore) -> QuickLookInputs {
    QuickLookInputs {
        workspace_id: WORKSPACE_ID.to_string(),
        environment: Some(environment.clone()),
        globals: Vec::new(),
        local_values: Vec::new(),
        secret_store: Arc::new(store),
    }
}

/// A workspace window with this crate wired up, drawn and active.
fn quick_look_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        crate::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (workspace, cx)
}

/// One open round: install `inputs`, dispatch `environment::QuickLook`, and let the
/// dialog's background status lookup land before handing the dialog back.
fn open(
    cx: &mut VisualTestContext,
    workspace: &Entity<Workspace>,
    inputs: QuickLookInputs,
) -> Entity<QuickLook> {
    cx.update(|_window, cx| install_inputs(cx, inputs));
    cx.dispatch_action(QuickLookAction);
    cx.run_until_parked();
    draw_window(cx);
    let quick_look = quick_look_of(workspace, cx)
        .expect("dispatching environment::QuickLook opens the quick-look");
    cx.run_until_parked();
    quick_look
}

/// The dialog the workspace's modal layer is showing, if the quick-look is open.
fn quick_look_of(
    workspace: &Entity<Workspace>,
    cx: &VisualTestContext,
) -> Option<Entity<QuickLook>> {
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace.modal_layer().active_modal::<QuickLook>()
        })
    })
}

/// Every displayed row of `quick_look`.
fn displayed_rows(quick_look: &Entity<QuickLook>, cx: &VisualTestContext) -> Vec<DisplayedRow> {
    cx.read(|cx| quick_look.read_with(cx, |quick_look, _| quick_look.displayed_rows()))
}

/// The row for `variable_key`.
fn row_for<'a>(rows: &'a [DisplayedRow], variable_key: &str) -> &'a DisplayedRow {
    rows.iter()
        .find(|row| row.key == variable_key)
        .expect("the variable is listed")
}

/// Every string the dialog displays, joined — what "the rendered text" means for a
/// leak assertion.
fn displayed_text(rows: &[DisplayedRow]) -> String {
    rows.iter()
        .flat_map(|row| [row.key.as_str(), row.shared.as_str(), row.local.as_str()])
        .collect::<Vec<_>>()
        .join(" ")
}

/// Turn `variable_key`'s reveal toggle the keyboard's way: focus the toggle's own
/// handle and press Enter, which GPUI turns into one click on the focused control.
fn reveal(quick_look: &Entity<QuickLook>, cx: &mut VisualTestContext, variable_key: &str) {
    let focus_handle = cx
        .read(|cx| {
            quick_look.read_with(cx, |quick_look, _| {
                quick_look.reveal_focus_handle(variable_key)
            })
        })
        .expect("a secret row has a reveal toggle");
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
}

#[gpui::test]
fn quick_look_masks_secrets(cx: &mut TestAppContext) {
    let environment = staging();
    let store = MemorySecretStore::new();
    let secret_key = secret_key_for(&environment, "api_token");
    store
        .set(&secret_key, &SecretValue::new(FAKE_SECRET))
        .expect("the fixture secret stores");
    let (workspace, cx) = quick_look_window(cx);
    let quick_look = open(cx, &workspace, inputs(&environment, store));

    let rows = displayed_rows(&quick_look, cx);
    let token = row_for(&rows, "api_token");
    assert_eq!(
        token.shared, NO_VALUE_LABEL,
        "a secret has no shared value (REQ-VAR-SHARE-02)"
    );
    assert_eq!(token.local, MASK, "a set secret renders the mask");
    assert!(
        !displayed_text(&rows).contains(FAKE_SECRET),
        "the value appears nowhere in the rendered text until reveal"
    );

    reveal(&quick_look, cx, "api_token");

    let rows = displayed_rows(&quick_look, cx);
    assert_eq!(
        row_for(&rows, "api_token").local,
        FAKE_SECRET,
        "the reveal toggle shows the value it fetched"
    );
}

#[gpui::test]
fn unset_secret_labelled_unset(cx: &mut TestAppContext) {
    let environment = staging();
    // The store holds no entry for `api_token`: the teammate never set the secret.
    let store = MemorySecretStore::new();
    let (workspace, cx) = quick_look_window(cx);
    let quick_look = open(cx, &workspace, inputs(&environment, store));

    let rows = displayed_rows(&quick_look, cx);
    let token = row_for(&rows, "api_token");
    assert_eq!(
        token.local, UNSET_LABEL,
        "a secret absent from the store renders 'unset'"
    );
    assert!(!token.local.is_empty(), "the label is not an empty cell");
    assert_ne!(token.local, MASK, "the label is not the mask");
}
