//! The Collections tree's keyboard navigation and the tabs its Enter opens (S39b): the
//! selection model — which row the arrows are on and how it moves through the tree — and
//! the five handlers behind Up, Down, Left, Right and Enter.
//!
//! The keys are bound here, in code, against [`COLLECTIONS_TREE_CONTEXT`] (the
//! `variable_autocomplete` shape): `assets/keymaps/` is the home of the PRD's command
//! actions (`pustman_actions`), while these are one component's navigation gestures —
//! the same split `ui::init` draws for its `TabBar` and menus.

use std::collections::BTreeMap;

use api_model::{ItemId, Request};
use gpui::{Entity, ScrollStrategy, UniformListScrollHandle, WeakEntity};
use request_editor::RequestEditor;
use ui::prelude::*;
use workspace::Workspace;
use workspace_store::ItemBody;

use crate::tree::{CollectionsTree, TreeRow};

/// The key context the tree answers the keyboard in; [`init`] binds the arrows and Enter
/// against it.
pub const COLLECTIONS_TREE_CONTEXT: &str = "CollectionsTree";

// Left and Right are this tree's own: a component action pair no other list answers.
gpui::actions!(
    collections,
    [
        /// Collapses the selected container — or moves the selection to its parent row.
        Collapse,
        /// Expands the selected container — or moves the selection to its first child row.
        Expand,
    ]
);

/// Bind the tree's five keys once per app, in [`COLLECTIONS_TREE_CONTEXT`].
pub fn init(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("up", SelectPrevious, Some(COLLECTIONS_TREE_CONTEXT)),
        gpui::KeyBinding::new("down", SelectNext, Some(COLLECTIONS_TREE_CONTEXT)),
        gpui::KeyBinding::new("left", Collapse, Some(COLLECTIONS_TREE_CONTEXT)),
        gpui::KeyBinding::new("right", Expand, Some(COLLECTIONS_TREE_CONTEXT)),
        gpui::KeyBinding::new("enter", Activate, Some(COLLECTIONS_TREE_CONTEXT)),
    ]);
}

/// The tree's keyboard state: which row the selection is on, the scroll handle that keeps
/// that row in view as the arrows move it, and the [`RequestEditor`] tabs Enter has
/// opened — keyed by the request's [`ItemId`], so one request owns one tab (REQ-UI-02).
pub(crate) struct Navigation {
    /// The selected row's identity. A row's identity is its [`TreeRow::element_id`] —
    /// the item's stable [`ItemId`], or a problem file's path — never its position, which
    /// shifts as folders open and close. [`None`] means the keyboard has not moved yet.
    selected: Option<ElementId>,
    /// The list's scroll state, so a selection that moves off screen scrolls back into
    /// view ([`ui::VirtualList::track_scroll`]).
    scroll_handle: UniformListScrollHandle,
    /// The tabs Enter has opened, keyed by request id. A weak handle per tab: closing the
    /// tab releases its editor and the next Enter opens a fresh one.
    open_tabs: BTreeMap<ItemId, WeakEntity<RequestEditor>>,
}

impl Navigation {
    /// No selection yet, and no tabs opened.
    pub(crate) fn new() -> Self {
        Self {
            selected: None,
            scroll_handle: UniformListScrollHandle::new(),
            open_tabs: BTreeMap::new(),
        }
    }

    /// The selected row's identity — what a row frame draws its selection against.
    pub(crate) fn selected_key(&self) -> Option<&ElementId> {
        self.selected.as_ref()
    }

    /// The list's scroll handle, for [`ui::VirtualList::track_scroll`].
    pub(crate) fn scroll_handle(&self) -> &UniformListScrollHandle {
        &self.scroll_handle
    }

    /// Where the row `key` sits in `rows`, if the rows the tree draws still hold it.
    pub(crate) fn index_of(&self, key: &ElementId, rows: &[TreeRow]) -> Option<usize> {
        rows.iter().position(|row| &row.element_id() == key)
    }

    /// Where the selection sits in `rows` — [`None`] while nothing is selected, and after
    /// a snapshot change dropped the selected row (which the arrows treat as "nothing
    /// selected yet").
    pub(crate) fn selected_index(&self, rows: &[TreeRow]) -> Option<usize> {
        self.selected
            .as_ref()
            .and_then(|key| self.index_of(key, rows))
    }

    /// Put the selection on the row `key` identifies and keep it in view.
    /// [`ScrollStrategy::Nearest`] scrolls only when the row is off screen.
    pub(crate) fn select(&mut self, key: ElementId, index: usize) {
        self.selected = Some(key);
        self.scroll_handle
            .scroll_to_item(index, ScrollStrategy::Nearest);
    }

    /// The [`RequestEditor`] Enter opened for `id`, if its tab is still alive.
    fn open_tab(&self, id: &ItemId) -> Option<Entity<RequestEditor>> {
        self.open_tabs.get(id).and_then(WeakEntity::upgrade)
    }

    /// Remember `editor` as `id`'s tab, dropping the entries whose tabs are gone so the
    /// map cannot grow past the requests the user has opened.
    fn remember_open_tab(&mut self, id: ItemId, editor: &Entity<RequestEditor>) {
        self.open_tabs.retain(|_, open| open.upgrade().is_some());
        self.open_tabs.insert(id, editor.downgrade());
    }
}

impl CollectionsTree {
    /// Up: the row above the selection. With nothing selected it enters the list at its
    /// end, the way [`Self::select_next`] enters at its start. The arrows stop at the
    /// tree's ends instead of wrapping around them (`Decision:`): a tree has a top and a
    /// bottom, and wrapping hides where the list ends.
    pub(crate) fn select_previous(
        &mut self,
        _: &SelectPrevious,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_selection(false, cx);
    }

    /// Down: the row under the selection — the first row when nothing is selected yet.
    pub(crate) fn select_next(
        &mut self,
        _: &SelectNext,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_selection(true, cx);
    }

    /// Left: collapse the selected container, or — on a request, or on a container that
    /// is already closed — move the selection to its parent row (the tree behaviour
    /// Zed's project panel and the file explorers agree on).
    pub(crate) fn collapse_or_parent(
        &mut self,
        _: &Collapse,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.navigation.selected_index(self.rows()) else {
            return;
        };
        let Some(row) = self.rows().get(index).cloned() else {
            return;
        };
        let depth = row.depth();
        match row {
            TreeRow::Collection {
                id,
                is_expanded: true,
                ..
            }
            | TreeRow::Folder {
                id,
                is_expanded: true,
                ..
            } => self.set_expanded(id, false, cx),
            _ => {
                if depth == 0 {
                    // A root row has no parent to move to.
                    return;
                }
                // The parent of a row is the nearest row above it one level out: the
                // rows in between are its parent's other subtrees.
                let parent = (0..index).rev().find(|candidate| {
                    self.rows()
                        .get(*candidate)
                        .is_some_and(|row| row.depth() + 1 == depth)
                });
                if let Some(parent_index) = parent {
                    self.select_index(parent_index, cx);
                }
            }
        }
    }

    /// Right: expand the selected container, or — on one that is already open — move the
    /// selection to its first child row. A request is a leaf and a problem row has no
    /// children: there is nothing to open.
    pub(crate) fn expand_or_child(
        &mut self,
        _: &Expand,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.navigation.selected_index(self.rows()) else {
            return;
        };
        let Some(row) = self.rows().get(index).cloned() else {
            return;
        };
        let depth = row.depth();
        match row {
            TreeRow::Collection {
                id,
                is_expanded: false,
                ..
            }
            | TreeRow::Folder {
                id,
                is_expanded: false,
                ..
            } => self.set_expanded(id, true, cx),
            TreeRow::Collection { .. } | TreeRow::Folder { .. } => {
                let has_child = self
                    .rows()
                    .get(index + 1)
                    .is_some_and(|row| row.depth() == depth + 1);
                if has_child {
                    self.select_index(index + 1, cx);
                }
            }
            TreeRow::Request { .. } | TreeRow::Problem { .. } => {}
        }
    }

    /// Enter: open the selected request's tab. On a container it opens and closes the
    /// container instead — the one confirmation a tree row can make (`Decision:`). A
    /// problem row confirms nothing: its error is already on the row.
    pub(crate) fn activate(&mut self, _: &Activate, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.navigation.selected_index(self.rows()) else {
            return;
        };
        let Some(row) = self.rows().get(index).cloned() else {
            return;
        };
        match row {
            TreeRow::Collection {
                id, is_expanded, ..
            }
            | TreeRow::Folder {
                id, is_expanded, ..
            } => self.set_expanded(id, !is_expanded, cx),
            TreeRow::Request { id, .. } => self.open_request(id, window, cx),
            TreeRow::Problem { .. } => {}
        }
    }

    /// One row's click: select it, and open it when it is a request — the one open path
    /// the keyboard's [`Self::activate`] shares (`Decision:` Postman opens a request on
    /// the first click). A container row selects only; its disclosure opens and closes it.
    pub(crate) fn click_row(
        &mut self,
        row_key: ElementId,
        request: Option<ItemId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.navigation.index_of(&row_key, self.rows()) {
            self.select_index(index, cx);
        }
        if let Some(id) = request {
            self.open_request(id, window, cx);
        }
    }

    /// Move the selection one row in `forward`'s direction, clamped at the tree's ends.
    fn move_selection(&mut self, forward: bool, cx: &mut Context<Self>) {
        let rows = self.rows();
        let last = match rows.len() {
            0 => return,
            length => length - 1,
        };
        let target = match self.navigation.selected_index(rows) {
            Some(index) if forward => (index + 1).min(last),
            Some(index) => index.saturating_sub(1),
            None if forward => 0,
            None => last,
        };
        self.select_index(target, cx);
    }

    /// Put the selection on the row at `index` and redraw.
    fn select_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows().get(index) else {
            return;
        };
        let key = row.element_id();
        self.navigation.select(key, index);
        cx.notify();
    }

    /// Enter's open: the request's tab in the workspace's center pane. One tab per
    /// request (REQ-UI-02): a request that already has a tab gets that tab activated and
    /// focused instead of a second one.
    ///
    /// `Decision:` the panel tracks the tabs it opens itself, keyed by [`ItemId`].
    /// `Pane`'s public API hands out the tabs' entity ids and the active tab only, so a
    /// tab another opener created — a restored session's — cannot be found by request id
    /// from here; the pane itself tolerates two tabs naming one item (its `Restore`
    /// says so), and the panel's own opens never make a second one.
    pub(crate) fn open_request(&mut self, id: ItemId, window: &mut Window, cx: &mut Context<Self>) {
        // The request the row names, straight out of the snapshot the rows came from.
        let request = match self.snapshot.items.get(&id).map(|item| &item.body) {
            Some(ItemBody::Request(Request::Http(request))) => Some(request.clone()),
            Some(ItemBody::Request(Request::Unsupported(_))) => None,
            // The row's file is gone since the last flatten; its story belongs to the
            // watcher (S42a).
            _ => return,
        };
        let Some(request) = request else {
            self.report_unopenable(id, window, cx);
            return;
        };
        let Some(Some(workspace)) = window.root::<Workspace>() else {
            // The panel draws inside a workspace window; without one there is no pane to
            // open a tab in.
            return;
        };
        let pane = workspace.read(cx).center_pane().clone();
        if let Some(editor) = self.navigation.open_tab(&id) {
            let entity_id = editor.entity_id();
            let open_at = pane
                .read(cx)
                .item_ids()
                .iter()
                .position(|tab| *tab == entity_id);
            if let Some(index) = open_at {
                pane.update(cx, |pane, cx| pane.set_active_index(index, cx));
                editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
                return;
            }
        }
        let editor = cx.new(|cx| RequestEditor::new(request, window, cx));
        self.navigation.remember_open_tab(id, &editor);
        pane.update(cx, |pane, cx| pane.add_tab_item(editor.clone(), cx));
        editor.update(cx, |editor, cx| editor.focus_url_bar(window, cx));
    }

    /// Enter on a request kind this build does not open: say so where the user is looking
    /// (AGENTS.md › Errors reach the user) — a keystroke that silently does nothing is a
    /// dead end.
    fn report_unopenable(&mut self, id: ItemId, window: &mut Window, cx: &mut Context<Self>) {
        let name = self
            .snapshot
            .items
            .get(&id)
            .map(|item| item.name())
            .unwrap_or_default();
        if let Some(Some(workspace)) = window.root::<Workspace>() {
            workspace.update(cx, |workspace, cx| {
                workspace.show_toast(
                    format!("{name} is not an HTTP request — this build opens HTTP requests only."),
                    Severity::Warning,
                    None,
                    cx,
                );
            });
        }
    }
}

#[cfg(test)]
#[path = "navigation_tests.rs"]
mod tests;
