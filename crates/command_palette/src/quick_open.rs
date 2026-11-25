//! Quick open (PRD REQ-UI-05, `Ctrl/Cmd-P`): a fuzzy finder over every request in the
//! open workspace — rows of "Collection / Folder / Name" with the request's method badge
//! — whose Enter opens the chosen request's tab (REQ-UI-02: one request owns one tab).
//!
//! `Decision:` the rows come from the open workspace's snapshot through
//! [`WorkspaceOpen`] — the state the app's open flows (`workspace::WorkspaceOpen`'s
//! folder dialog, launch reopen and `OpenRecent`, all landing in
//! [`WorkspaceOpen::open_folder`]) already install. Nothing here takes a hand-installed
//! input, so the shipped app lists what an opened folder actually holds and a window
//! with no open workspace is a state the user is told about (AGENTS.md › errors reach
//! the user), not an empty list.
//!
//! `Decision:` a row carries display text only — its id, its "Collection / Folder /
//! Name" path, its method badge and one cached lowercased sort key — never a clone of
//! the request behind it. Confirming reads the request from the snapshot as it is *then*
//! ([`open_request`]): the listing can outlive the snapshot it was built from (an
//! external change lands under an open picker, REQ-COLLAB-01), and opening that stale
//! clone would edit and send a request the workspace no longer holds.
//!
//! `Decision:` the display path is the chain of containers the `order` arrays name
//! (REQ-WSP-01) — the same parentage `collections_panel`'s tree draws — joined with
//! ` / `. It is derived here rather than reused because `collections_panel`'s
//! `parent_map`/`container_order` are that crate's internals and this crate may not
//! depend on it; folding the walk into `workspace_store` is a later refactor (S50e
//! folds both open paths into `request_editor::open_item`). Rows sort by their cached
//! lowercased path, so an empty query lists the workspace the way the tree names it and
//! two runs never reorder. Matching runs on the display text itself — the scorer is
//! case-insensitive and its camelCase word-start bonus must see the original casing.
//!
//! `Decision:` every request kind the snapshot holds is listed — a WebSocket or gRPC
//! file is still a request the user may be hunting — but only HTTP requests open:
//! confirming any other kind names it in a warning toast (the rule
//! `collections_panel`'s tree follows), instead of silently doing nothing.

use std::collections::{BTreeMap, BTreeSet};

use api_model::{ItemId, Request};
use gpui::{AnyElement, App, Context, DismissEvent, ElementId, Entity, SharedString, Window};
use request_editor::RequestEditor;
use ui::prelude::*;
use workspace::{Pane, Workspace, WorkspaceOpen};
use workspace_store::{ItemBody, WorkspaceSnapshot};

use crate::fuzzy;
use crate::picker::{Picker, PickerDelegate};

#[cfg(test)]
#[path = "quick_open_tests.rs"]
mod tests;

/// What a window with no open workspace is told: there is nothing to search yet
/// ([`WorkspaceOpen::snapshot`] is [`None`] until a folder lands).
const NO_WORKSPACE_MESSAGE: &str =
    "Quick open needs an open workspace — open a workspace folder first";

/// What quick open says instead of an empty picker: nothing to choose is not a list the
/// user should search (the shape `crate::recent` follows).
const EMPTY_MESSAGE: &str = "There are no requests to open";

/// What a request kind this build cannot open is told, with the row the user picked.
fn unopenable_message(display: &str) -> String {
    format!("{display} is not an HTTP request — this build opens HTTP requests only.")
}

/// What confirming a request the workspace no longer holds shows: the row was listed
/// from a snapshot that has since been replaced. `pub(crate)`: the tests pin it.
pub(crate) fn gone_message(display: &str) -> String {
    format!("{display} is no longer in the workspace — it may have been deleted or renamed")
}

/// One request the picker offers: its identity and the display text its row draws —
/// the "Collection / Folder / Name" path, the method badge, and the lowercased key the
/// rows sort by. The request itself is never carried (see the module docs).
pub struct QuickOpenEntry {
    /// The request's stable [`ItemId`] — the row's element id and the confirm's lookup.
    pub id: ItemId,
    /// "Collection / Folder / Name" — what the row shows and what the query matches.
    pub display: String,
    /// The method badge text: the HTTP verb, or the `type` tag of a kind this build does
    /// not model.
    pub method: String,
    /// [`Self::display`], lowercased once: the rows' sort key (and the reason the sort
    /// never re-lowercases per comparison).
    pub key: String,
}

/// The delegate behind quick open: the workspace's requests, fuzzy-filtered by
/// [`fuzzy`] against the query, opened in their tabs on confirm.
pub struct QuickOpenDelegate {
    entries: Vec<QuickOpenEntry>,
    /// Indices into `entries`, in rank order — what the list draws.
    matches: Vec<usize>,
    selected: usize,
}

impl QuickOpenDelegate {
    /// Quick open over `entries` (see [`request_entries`]).
    pub fn new(entries: Vec<QuickOpenEntry>) -> Self {
        let matches = (0..entries.len()).collect();
        Self {
            entries,
            matches,
            selected: 0,
        }
    }

    /// The entries behind the rows the list draws, top first.
    pub fn listed(&self) -> Vec<&QuickOpenEntry> {
        self.matches
            .iter()
            .filter_map(|entry_index| self.entries.get(*entry_index))
            .collect()
    }

    /// The entry behind match `index`, if there is one.
    fn entry_at(&self, index: usize) -> Option<&QuickOpenEntry> {
        self.matches
            .get(index)
            .and_then(|entry_index| self.entries.get(*entry_index))
    }
}

impl PickerDelegate for QuickOpenDelegate {
    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected
    }

    fn set_selected_index(&mut self, index: usize, cx: &mut Context<Picker<Self>>) {
        self.selected = index;
        cx.notify();
    }

    fn update_matches(&mut self, query: &str, cx: &mut Context<Picker<Self>>) {
        self.matches = fuzzy::rank(
            query,
            self.entries.iter().map(|entry| entry.display.as_str()),
        )
        .into_iter()
        .map(|string_match| string_match.candidate_index)
        .collect();
        cx.notify();
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        let Some(entry) = self.entry_at(self.selected) else {
            return;
        };
        let id = entry.id;
        let display = entry.display.clone();
        // Dismiss first: the modal layer restores focus as it goes. The open runs one
        // effect later (`Window::defer`, the shape `crate::palette` dispatches with), so
        // it lands against the restored focus and the tab keeps it — and it reads the
        // request from the snapshot as it is then, never from this listing (see the
        // module docs).
        cx.emit(DismissEvent);
        window.defer(cx, move |window, cx| open_request(id, display, window, cx));
    }

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {}

    fn render_match(
        &self,
        index: usize,
        _selected: bool,
        _window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let Some(entry) = self.entry_at(index) else {
            return div().into_any_element();
        };
        let display = entry.display.clone();
        h_flex()
            .w_full()
            .min_w_0()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(
                div()
                    .id(format!("quick-open-method-{}", entry.id))
                    .flex_none()
                    .child(
                        Label::new(entry.method.clone())
                            .size(LabelSize::Small)
                            .color(method_color(&entry.method, cx)),
                    ),
            )
            .child(
                div()
                    .id(format!("quick-open-path-{}", entry.id))
                    .min_w_0()
                    .flex_1()
                    .tooltip(Tooltip::text(display.clone()).builder())
                    .child(Label::new(display).truncate()),
            )
            .into_any_element()
    }

    fn match_key(&self, index: usize) -> ElementId {
        // The request's [`ItemId`] is the row's identity — never its rank, which the
        // query changes.
        self.entry_at(index)
            .map(|entry| ElementId::from(format!("quick-open-item-{}", entry.id)))
            .unwrap_or_else(|| ElementId::from("quick-open-row"))
    }

    fn placeholder_text(&self) -> SharedString {
        SharedString::new_static("Search requests")
    }
}

/// Open `id`'s request in the window's center pane — or focus the tab another opener
/// already holds for it — reading the request from the workspace's snapshot as it is
/// NOW (see the module docs). A request the workspace no longer holds, or one of a kind
/// this build has no editor for, is named in a warning toast and nothing opens
/// (AGENTS.md › errors reach the user).
fn open_request(id: ItemId, display: String, window: &mut Window, cx: &mut App) {
    let Some(Some(workspace)) = window.root::<Workspace>() else {
        // Quick open runs inside a workspace window; without one there is no pane to
        // open a tab in.
        return;
    };
    let opener = WorkspaceOpen::for_window(workspace.entity_id(), cx);
    let located = opener.as_ref().and_then(|opener| {
        let snapshot = opener.read(cx).snapshot()?;
        match snapshot.items.get(&id).map(|item| &item.body) {
            Some(ItemBody::Request(request)) => Some(request.clone()),
            _ => None,
        }
    });
    let http = match located {
        Some(Request::Http(http)) => http,
        Some(Request::Unsupported(_)) => {
            workspace.update(cx, |workspace, cx| {
                workspace.show_toast(unopenable_message(&display), Severity::Warning, None, cx);
            });
            return;
        }
        None => {
            workspace.update(cx, |workspace, cx| {
                workspace.show_toast(gone_message(&display), Severity::Warning, None, cx);
            });
            return;
        }
    };
    let pane = workspace.read(cx).center_pane().clone();
    if let Some(editor) = find_editor(&pane, http.id, cx) {
        editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
        return;
    }
    let editor = cx.new(|cx| RequestEditor::new(http, window, cx));
    pane.update(cx, |pane, cx| pane.add_tab_item(editor.clone(), cx));
    editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
}

/// The [`RequestEditor`] tab already showing `id`'s request, with its tab made active —
/// or [`None`] when no tab holds it. One request owns one tab however it was opened
/// (REQ-UI-02), so the match is by the item id a request tab persists under: the tree's
/// opens, a restored session's and a history open's alike.
fn find_editor(pane: &Entity<Pane>, id: ItemId, cx: &mut App) -> Option<Entity<RequestEditor>> {
    pane.update(cx, |pane, cx| {
        pane.activate_item_with_persistent_id(&id.to_string(), cx)
    })
    .and_then(|view| view.downcast::<RequestEditor>().ok())
}

/// The workspace's requests as quick open's rows: every request the snapshot holds, with
/// the "Collection / Folder / Name" path its containers order it under (see the module
/// docs), sorted by that path. The rows carry display text only — no request bodies.
fn request_entries(snapshot: &WorkspaceSnapshot) -> Vec<QuickOpenEntry> {
    // Which container orders each item — what "the item's parent" means here. A child
    // two parents both claim (a merge artifact) keeps the first, in id order.
    let mut parents: BTreeMap<ItemId, ItemId> = BTreeMap::new();
    for (id, item) in &snapshot.items {
        for child in container_order(&item.body) {
            parents.entry(*child).or_insert(*id);
        }
    }
    let mut entries: Vec<QuickOpenEntry> = snapshot
        .items
        .iter()
        .filter_map(|(id, item)| match &item.body {
            ItemBody::Request(request) => Some((*id, item.name(), request)),
            _ => None,
        })
        .map(|(id, name, request)| {
            let mut display = container_path(id, &parents, snapshot);
            if !display.is_empty() {
                display.push_str(" / ");
            }
            display.push_str(&name);
            let method = method_badge(request);
            let key = display.to_lowercase();
            QuickOpenEntry {
                id,
                display,
                method,
                key,
            }
        })
        .collect();
    entries.sort_by(|left, right| {
        left.key
            .cmp(&right.key)
            .then_with(|| left.display.cmp(&right.display))
            .then_with(|| left.id.cmp(&right.id))
    });
    entries
}

/// The names of `id`'s container chain, outermost first ("Collection / Folder"). A
/// request no container's `order` claims is a root of its own (the merge artifact the
/// tree draws as an orphan), so its path is empty and its name stands alone.
fn container_path(
    id: ItemId,
    parents: &BTreeMap<ItemId, ItemId>,
    snapshot: &WorkspaceSnapshot,
) -> String {
    let mut segments = Vec::new();
    let mut current = id;
    let mut walked = BTreeSet::new();
    while let Some(parent) = parents.get(&current) {
        // A cycle with no entrance (a rename/rename merge artifact) must not spin.
        if !walked.insert(*parent) {
            break;
        }
        match snapshot
            .items
            .get(parent)
            .map(|item| (&item.body, item.name()))
        {
            Some((ItemBody::Collection(_) | ItemBody::Folder(_), name)) => segments.push(name),
            _ => break,
        }
        current = *parent;
    }
    segments.reverse();
    segments.join(" / ")
}

/// The child ids a container orders (REQ-WSP-01) — the same arrays
/// `collections_panel`'s tree flattens; see the module docs on why the walk lives here.
fn container_order(body: &ItemBody) -> &[ItemId] {
    match body {
        ItemBody::Collection(collection) => &collection.order,
        ItemBody::Folder(folder) => &folder.order,
        _ => &[],
    }
}

/// The method badge text of a request: its HTTP method, or the `type` tag of a request
/// kind this build does not model — a WebSocket or gRPC file still says what it is (the
/// text `collections_panel`'s tree badges the same rows with).
fn method_badge(request: &Request) -> String {
    match request {
        Request::Http(request) => request.method.as_str().to_owned(),
        Request::Unsupported(raw) => raw
            .get("type")
            .and_then(|kind| kind.as_str())
            .unwrap_or("request")
            .to_owned(),
    }
}

/// Wire quick open to `quick_open::Toggle` (PRD REQ-UI-05): list the open workspace's
/// requests over the active workspace — or close it, [`Workspace::toggle_modal`] toggles.
pub fn init(cx: &mut App) {
    cx.on_action(toggle_quick_open);
}

/// Open quick open in the active window. `quick_open::Toggle` reaches this through
/// GPUI's app-level action listeners; like `crate::palette`'s toggle it is deferred,
/// because the dispatch happens inside the window's own update and a window cannot be
/// updated while it is on the update stack.
fn toggle_quick_open(_: &pustman_actions::quick_open::Toggle, cx: &mut App) {
    let Some(window_handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        match window_handle.update(cx, |root, window, cx| {
            let Ok(workspace_handle) = root.downcast::<Workspace>() else {
                return;
            };
            // Read from what the app already installs: this window's [`WorkspaceOpen`],
            // whose snapshot the open flows fill (see the module docs).
            let listing = WorkspaceOpen::for_window(workspace_handle.entity_id(), cx)
                .as_ref()
                .and_then(|opener| opener.read(cx).snapshot().map(request_entries));
            workspace_handle.update(cx, |workspace, cx| match listing {
                None => {
                    workspace.show_toast(NO_WORKSPACE_MESSAGE, Severity::Info, None, cx);
                }
                Some(entries) if entries.is_empty() => {
                    workspace.show_toast(EMPTY_MESSAGE, Severity::Info, None, cx);
                }
                Some(entries) => {
                    workspace.toggle_modal(window, cx, |window, cx| {
                        Picker::new(QuickOpenDelegate::new(entries), window, cx)
                    });
                }
            });
        }) {
            Ok(()) => {}
            // The window closed between the keystroke and this callback: there is no
            // workspace left to list requests for and nobody left to tell. The discard
            // is spelled out, not a result dropped on the floor.
            Err(_window_gone) => {}
        }
    });
}
