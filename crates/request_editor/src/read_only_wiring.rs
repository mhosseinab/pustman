//! Editing is disabled in a read-only workspace (REQ-WSP-04): this module is the one
//! place that pushes the flag — [`workspace::read_only::is_read_only`], which S41c
//! derives from the open workspace's `format_version` — into the request editor's text
//! inputs through `text_input`'s read-only mode. Typing, cut and paste are refused there
//! while motion and selection keep working, so a workspace that refuses every write
//! (`workspace_store::save_item` turns the file away) cannot grow a draft behind a save
//! that will never land. The flag has one source of truth: this module only reads it.
//!
//! `Decision:` the wiring pushes the flag when a [`RequestEditor`] is created and again
//! whenever the open workspace's flag changes (any [`Workspace`] notification follows
//! `read_only`'s own sync, which sets the flag and then notifies): a workspace that
//! opens read-only has its tabs read-only from the moment they exist, and one opened
//! writable after a read-only one gives the same tabs their editing back. The flag is
//! pushed, not polled — an input cannot ask for it and rendering must not mutate state —
//! and the push is guarded by the value last applied, so ordinary workspace
//! notifications cost one comparison.
//!
//! `Decision:` the reach of this step is the request editor's own field API: the URL
//! field, every cell of the Params, Headers and urlencoded tables, the raw body area and
//! the auth tab's bearer field go read-only. The auth tab's basic and API-key fields,
//! the Settings tab's fields and the Body tab's form-data rows have no accessor from
//! this step's Edits set, and rows an edit adds or a cURL import rebuilds are created
//! after the push — those stay editable until their owners expose them, and nothing they
//! type can be saved (above).

use gpui::{App, Entity, Global, Subscription, WeakEntity};
use text_input::{TextArea, TextInput};
use workspace::Workspace;
use workspace::read_only::is_read_only;

use crate::RequestEditor;

#[cfg(test)]
#[path = "read_only_wiring_tests.rs"]
mod tests;

/// The one wiring for the whole app and the registry of the request editors it pushes
/// the flag into (the `workspace::read_only::Wiring` shape, with its double-init guard).
#[derive(Default)]
struct Wiring {
    started: bool,
    /// The flag value last pushed to every registered editor's inputs. A workspace
    /// notification compares against it, so the push runs only when the flag moved.
    applied: Option<bool>,
    /// Every request editor built since `init` — dead handles are dropped on the next
    /// push. [`WeakEntity`] because a closed tab must not be kept alive by this.
    editors: Vec<WeakEntity<RequestEditor>>,
    _subscriptions: Vec<Subscription>,
}

impl Global for Wiring {}

/// Wire the read-only push into the app: every new [`RequestEditor`] registers here and
/// gets the current flag, and every [`Workspace`] notification re-pushes it when it
/// changed. Must run in `init` before any request tab exists.
pub fn init(cx: &mut App) {
    if cx.default_global::<Wiring>().started {
        return;
    }
    cx.default_global::<Wiring>().started = true;
    let on_new_editor = cx.observe_new::<RequestEditor>(|editor, _window, cx| {
        let entity = cx.entity();
        cx.global_mut::<Wiring>().editors.push(entity.downgrade());
        let read_only = is_read_only(cx);
        apply_to(editor, read_only, cx);
    });
    let on_new_workspace = cx.observe_new::<Workspace>(|_workspace, _window, cx| {
        let subscription = cx.observe_self(|_workspace, cx| sync_editors(cx));
        cx.global_mut::<Wiring>()._subscriptions.push(subscription);
    });
    cx.default_global::<Wiring>()
        ._subscriptions
        .push(on_new_editor);
    cx.default_global::<Wiring>()
        ._subscriptions
        .push(on_new_workspace);
}

/// Push the flag into every registered editor's inputs when it differs from the value
/// last applied (module docs).
fn sync_editors(cx: &mut App) {
    let read_only = is_read_only(cx);
    {
        let wiring = cx.global_mut::<Wiring>();
        if wiring.applied == Some(read_only) {
            return;
        }
        wiring.applied = Some(read_only);
    }
    let editors: Vec<Entity<RequestEditor>> = cx
        .global::<Wiring>()
        .editors
        .iter()
        .filter_map(WeakEntity::upgrade)
        .collect();
    for editor in &editors {
        editor.update(cx, |editor, cx| apply_to(editor, read_only, cx));
    }
}

/// Set `read_only` on every text input of `editor` this crate can reach (module docs).
fn apply_to(editor: &RequestEditor, read_only: bool, cx: &mut App) {
    let (inputs, areas) = reachable_inputs(editor, cx);
    for input in inputs {
        input.update(cx, |input, cx| input.set_read_only(read_only, cx));
    }
    for area in areas {
        area.update(cx, |area, cx| area.set_read_only(read_only, cx));
    }
}

/// The text fields behind the editor's public field API (module docs): the URL field,
/// the Params, Headers and urlencoded tables' cells, the raw body area and the auth
/// tab's bearer field.
fn reachable_inputs(
    editor: &RequestEditor,
    cx: &App,
) -> (Vec<Entity<TextInput>>, Vec<Entity<TextArea>>) {
    let mut inputs = Vec::new();
    inputs.push(editor.url_bar().read(cx).url_input().clone());
    let tables = [
        editor.params().read(cx).query_table().clone(),
        editor.params().read(cx).path_table().clone(),
        editor.headers().read(cx).table().clone(),
        editor.body_tab().read(cx).urlencoded_table().clone(),
    ];
    for table in tables {
        inputs.extend(
            table
                .read(cx)
                .cell_inputs()
                .into_iter()
                .map(|(_, input)| input),
        );
    }
    inputs.push(editor.auth_tab().read(cx).bearer_token_input().clone());
    let areas = vec![editor.body_tab().read(cx).raw_text_area().clone()];
    (inputs, areas)
}
