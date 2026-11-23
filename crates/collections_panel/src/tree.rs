//! The Collections panel's tree (PRD REQ-UI-01): the [`WorkspaceSnapshot`]'s collections,
//! folders and requests as a virtualized tree of [`TreeRow`]s, one row per item the
//! snapshot can see.
//!
//! Row order is the `order` arrays' (REQ-WSP-01) — a parent stores its children's ids in
//! display order and the tree walks those, never the name order or the directory order.
//! Containers expand and collapse; every request row carries its method badge. A file the
//! snapshot could not read draws its error badge after the items, with the problem's own
//! words beside it (REQ-COLLAB-02; the detail UI is S42b).
//!
//! The rows are virtualized through [`VirtualList`] (REQ-NFR-02): the tree flattens the
//! visible rows once per structural change — an expand, a collapse, a new snapshot — and
//! drawing lays out only the rows the window can show.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use api_model::{ItemId, Request};
use gpui::{
    AnyElement, ClickEvent, Context, Div, ElementId, FocusHandle, Focusable, InteractiveText,
    Render, SharedString, Styled, StyledText, Window,
};
use ui::prelude::*;
use workspace::{DockPosition, Panel};
use workspace_store::{ItemBody, ItemProblem, ItemProblemKind, WorkspaceSnapshot};

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;

/// The panel's stable id: its dock selector entry and its hitbox.
pub const PANEL_ID: &str = "collections";

/// One row's height — the redesign pack's compact row (`design_tokens.json` › density),
/// which is what a navigation list of one-line rows draws at.
const ROW_HEIGHT_PX: f32 = 32.;

/// The width of a row's method column: fixed so the names line up down the tree.
const METHOD_COLUMN_PX: f32 = 48.;

/// The width of a row's disclosure column: fixed so a row's name starts in the same
/// place whether its row expands and collapses or is a leaf.
const DISCLOSURE_COLUMN_PX: f32 = 24.;

/// How much each level of nesting indents a row.
const INDENT_PX: f32 = 16.;

/// One row of the collections tree.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeRow {
    /// A collection: one of the tree's roots.
    Collection {
        id: ItemId,
        name: SharedString,
        is_expanded: bool,
    },
    /// A folder `depth` levels under its collection.
    Folder {
        id: ItemId,
        depth: usize,
        name: SharedString,
        is_expanded: bool,
    },
    /// A request `depth` levels under its collection: the leaf its method badge names.
    Request {
        id: ItemId,
        depth: usize,
        name: SharedString,
        method: SharedString,
    },
    /// A file the snapshot could not read, with the words its error badge (`summary`)
    /// and its tooltip (`detail`) carry.
    Problem {
        path: SharedString,
        summary: SharedString,
        detail: SharedString,
    },
}

impl TreeRow {
    /// How far right the row is indented: 0 at the roots.
    pub fn depth(&self) -> usize {
        match self {
            Self::Collection { .. } | Self::Problem { .. } => 0,
            Self::Folder { depth, .. } | Self::Request { depth, .. } => *depth,
        }
    }

    /// The row's label: an item's name, or the file a problem is about.
    pub fn name(&self) -> &str {
        match self {
            Self::Collection { name, .. }
            | Self::Folder { name, .. }
            | Self::Request { name, .. } => name,
            Self::Problem { path, .. } => path,
        }
    }

    /// The method badge text of a request row — [`None`] for rows that are not requests.
    pub fn method(&self) -> Option<&str> {
        match self {
            Self::Request { method, .. } => Some(method),
            _ => None,
        }
    }

    /// The row's element id: the item's stable [`ItemId`] (or a problem file's path),
    /// never the row's position, which shifts as rows expand and collapse.
    pub fn element_id(&self) -> ElementId {
        match self {
            Self::Collection { id, .. } | Self::Folder { id, .. } | Self::Request { id, .. } => {
                item_element_id(*id)
            }
            Self::Problem { path, .. } => problem_element_id(path),
        }
    }
}

/// One item row's element id: the item's stable [`ItemId`], never its row position.
fn item_element_id(id: ItemId) -> ElementId {
    ElementId::from(format!("tree-item-{id}"))
}

/// One problem row's element id: the path of the file it is about, which is the one
/// stable identity an unreadable file has.
fn problem_element_id(path: &str) -> ElementId {
    ElementId::from(format!("tree-problem-{path}"))
}

/// The left-dock Collections panel: the virtualized tree over one workspace snapshot.
pub struct CollectionsTree {
    snapshot: WorkspaceSnapshot,
    /// The containers the user has opened. Keyed by [`ItemId`] and kept across snapshot
    /// updates, so an external change never collapses the tree under the pointer (S42a).
    expanded: BTreeSet<ItemId>,
    /// The rows the last flatten produced. Read-only until the next flatten, so drawing
    /// clones the handle instead of re-walking the snapshot.
    rows: Arc<Vec<TreeRow>>,
    focus_handle: FocusHandle,
}

impl CollectionsTree {
    /// The tree over `snapshot`. A fresh tree starts collapsed: a workspace can hold
    /// hundreds of collections and the user opens the one they came for.
    pub fn new(snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) -> Self {
        // The panel is one tab stop, so the keyboard can reach the tree (S39b gives it
        // the arrow-key selection on top).
        let focus_handle = cx.focus_handle().tab_stop(true);
        let rows = Arc::new(visible_rows(&snapshot, &BTreeSet::new()));
        Self {
            snapshot,
            expanded: BTreeSet::new(),
            rows,
            focus_handle,
        }
    }

    /// The rows the tree draws.
    pub fn rows(&self) -> &[TreeRow] {
        self.rows.as_slice()
    }

    /// Show a different snapshot — the workspace was opened, saved or changed on disk.
    pub fn set_snapshot(&mut self, snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) {
        self.snapshot = snapshot;
        self.reflatten(cx);
    }

    /// Open or close one container. Recording the state of a leaf is harmless: it draws
    /// no disclosure and has no children to hold back.
    pub fn set_expanded(&mut self, id: ItemId, is_expanded: bool, cx: &mut Context<Self>) {
        if is_expanded {
            self.expanded.insert(id);
        } else {
            self.expanded.remove(&id);
        }
        self.reflatten(cx);
    }

    /// Flip one container between open and closed — its disclosure's click.
    pub fn toggle_expanded(&mut self, id: ItemId, cx: &mut Context<Self>) {
        let is_expanded = !self.expanded.contains(&id);
        self.set_expanded(id, is_expanded, cx);
    }

    /// Re-derive the rows after a structural change (an expand, a collapse, a new
    /// snapshot) and redraw. Never called per frame (`references/performance.md`).
    fn reflatten(&mut self, cx: &mut Context<Self>) {
        self.rows = Arc::new(visible_rows(&self.snapshot, &self.expanded));
        cx.notify();
    }

    /// One row's content — the wrapper `VirtualList` draws around it carries the row's
    /// stable key and test selector.
    fn render_row(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        match self.rows.get(index) {
            Some(TreeRow::Collection {
                id,
                name,
                is_expanded,
            }) => self.render_branch(*id, 0, name.clone(), *is_expanded, cx),
            Some(TreeRow::Folder {
                id,
                depth,
                name,
                is_expanded,
            }) => self.render_branch(*id, *depth, name.clone(), *is_expanded, cx),
            Some(TreeRow::Request {
                id,
                depth,
                name,
                method,
            }) => self.render_request(*id, *depth, name.clone(), method.clone(), cx),
            Some(TreeRow::Problem {
                path,
                summary,
                detail,
            }) => self.render_problem(path.clone(), summary.clone(), detail.clone(), cx),
            // The list is told the row count of the same flatten it draws keys from, so
            // an out-of-range index cannot occur — and must not panic if it ever does.
            None => div().into_any_element(),
        }
    }

    /// The frame every item row shares: one row's height, one indent per depth, and the
    /// columns' fixed widths so the names line up down the tree. The row's element id and
    /// test selector are the wrapper `VirtualList` draws around it.
    fn row_frame(&self, depth: usize, cx: &App) -> Div {
        h_flex()
            .w_full()
            .h(px_to_rems(ROW_HEIGHT_PX))
            .min_w_0()
            .items_center()
            .gap(DynamicSpacing::Base01.rems(cx))
            .pl(px_to_rems(INDENT_PX * depth as f32))
    }

    /// A collection's or folder's row: the disclosure, the name, and no method badge.
    fn render_branch(
        &self,
        id: ItemId,
        depth: usize,
        name: SharedString,
        is_expanded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row_id = item_element_id(id);
        self.row_frame(depth, cx)
            .child(self.disclosure(id, is_expanded, name.clone(), cx))
            .child(self.method_column(&row_id, None, cx))
            .child(row_label(
                name,
                ElementId::from(format!("{row_id}-name")),
                LabelFont::Ui,
                TextSize::Small,
                Color::Default,
                cx,
            ))
            .into_any_element()
    }

    /// A request row: the method badge and the name. Its disclosure column stays empty —
    /// a request has no children to open.
    fn render_request(
        &self,
        id: ItemId,
        depth: usize,
        name: SharedString,
        method: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let row_id = item_element_id(id);
        self.row_frame(depth, cx)
            .child(div().flex_none().w(px_to_rems(DISCLOSURE_COLUMN_PX)))
            .child(self.method_column(&row_id, Some(method), cx))
            .child(row_label(
                name,
                ElementId::from(format!("{row_id}-name")),
                LabelFont::Ui,
                TextSize::Small,
                Color::Default,
                cx,
            ))
            .into_any_element()
    }

    /// A problem row: the error badge and what is wrong with which file.
    fn render_problem(
        &self,
        path: SharedString,
        summary: SharedString,
        detail: SharedString,
        cx: &App,
    ) -> AnyElement {
        let row_id = problem_element_id(&path);
        let badge_selector = format!("{row_id}-badge");
        h_flex()
            .w_full()
            .h(px_to_rems(ROW_HEIGHT_PX))
            .min_w_0()
            .items_center()
            .gap(DynamicSpacing::Base03.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .child(
                div()
                    .flex_none()
                    .debug_selector(move || badge_selector)
                    .child(Badge::new(summary).severity(Severity::Error)),
            )
            .child(row_label(
                format!("{path} — {detail}").into(),
                ElementId::from(format!("{row_id}-name")),
                LabelFont::Ui,
                TextSize::Small,
                Color::Muted,
                cx,
            ))
            .into_any_element()
    }

    /// A container's disclosure: the one control that opens and closes it. Its tooltip
    /// names the row and the verb, which is also the button's accessible label.
    fn disclosure(
        &self,
        id: ItemId,
        is_expanded: bool,
        name: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let icon = if is_expanded {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        };
        let verb = if is_expanded { "Collapse" } else { "Expand" };
        div()
            .flex_none()
            .w(px_to_rems(DISCLOSURE_COLUMN_PX))
            .child(
                IconButton::new(
                    ElementId::from(format!("tree-toggle-{id}")),
                    icon,
                    Tooltip::text(format!("{verb} {name}")),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.toggle_expanded(id, cx);
                })),
            )
            .into_any_element()
    }

    /// The method column: the request's method (or the `type` of a request kind this
    /// build does not model) in the content font, or nothing on a row that is not a
    /// request.
    fn method_column(
        &self,
        row_id: &ElementId,
        method: Option<SharedString>,
        cx: &App,
    ) -> AnyElement {
        let column = div().flex_none().w(px_to_rems(METHOD_COLUMN_PX)).min_w_0();
        match method {
            Some(method) => {
                let color = method_color(&method, cx);
                column
                    .child(row_label(
                        method,
                        ElementId::from(format!("{row_id}-method")),
                        LabelFont::Buffer,
                        TextSize::XSmall,
                        color,
                        cx,
                    ))
                    .into_any_element()
            }
            None => column.into_any_element(),
        }
    }
}

impl Render for CollectionsTree {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Every element id in this panel derives from its focus handle — the panel's
        // identity — so two panels can never collide in one frame (the shape
        // `command_palette::Picker` builds its ids with).
        let panel_element_id = ElementId::from(&self.focus_handle);
        if self.rows.is_empty() {
            return v_flex()
                .id(panel_element_id)
                .size_full()
                .font_ui(cx)
                .child(
                    EmptyState::new(
                        "collections-empty-state",
                        IconName::Folder,
                        "No collections yet",
                    )
                    .description("The collections of the open workspace appear here."),
                )
                .into_any_element();
        }

        let rows = Arc::clone(&self.rows);
        let list = VirtualList::new(
            ElementId::NamedChild(Arc::new(panel_element_id.clone()), "rows".into()),
            rows.len(),
            move |index| {
                rows.get(index).map_or_else(
                    // The list is told the row count of the same flatten it draws keys
                    // from, so this is unreachable — and must not panic if it ever is
                    // not (#111's fallback shape).
                    || ElementId::from(format!("tree-row-fallback-{index}")),
                    TreeRow::element_id,
                )
            },
            cx.processor(|this, index: usize, _window, cx| this.render_row(index, cx)),
        );
        let list_focused = self.focus_handle.contains_focused(window, cx);

        v_flex()
            .id(panel_element_id)
            .size_full()
            .font_ui(cx)
            .p(DynamicSpacing::Base08.rems(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    // The list's focus style: the border is always drawn (transparent)
                    // so the focus ring shows without moving the rows' layout (the
                    // `console` row's focus-ring shape, #157).
                    .border_1()
                    .rounded_sm()
                    .border_color(cx.theme().colors().border_transparent)
                    .when(list_focused, |list_area| {
                        list_area.border_color(cx.theme().colors().border_focused)
                    })
                    .child(list),
            )
            .into_any_element()
    }
}

impl Focusable for CollectionsTree {
    /// The panel is one tab stop; S39b gives it the arrow-key selection.
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Panel for CollectionsTree {
    fn panel_id(&self) -> &'static str {
        PANEL_ID
    }

    fn title(&self, _cx: &App) -> SharedString {
        SharedString::new_static("Collections")
    }

    fn icon(&self) -> IconName {
        IconName::Folder
    }

    fn position(&self, _cx: &App) -> DockPosition {
        DockPosition::Left
    }
}

/// Which font a row's label draws in: the UI font for names, the content font (the
/// pack's monospace) for method badges.
#[derive(Clone, Copy)]
enum LabelFont {
    Ui,
    Buffer,
}

/// Text that can overflow a row: truncated, with the full value in its hover tooltip
/// when — and only when — the drawn text is not the whole of it (the History rows'
/// target's shape, #235).
fn row_label(
    text: SharedString,
    id: ElementId,
    font: LabelFont,
    size: TextSize,
    color: Color,
    cx: &App,
) -> AnyElement {
    let full_text = text.clone();
    let styled = StyledText::new(text);
    // The layout the text fills once the row lays it out — read at hover time to see
    // what was actually drawn.
    let drawn = styled.layout().clone();
    let text_id = ElementId::from(format!("{id}-text"));
    let frame = match font {
        LabelFont::Ui => div().font_ui(cx),
        LabelFont::Buffer => div().font_buffer(cx),
    };
    frame
        .id(id.clone())
        .debug_selector(move || id.to_string())
        .flex_1()
        .min_w_0()
        .text_size(size.rems(cx))
        .text_color(color.color(cx))
        .truncate()
        .child(
            InteractiveText::new(text_id, styled).tooltip(move |_index, _window, cx| {
                if drawn.text() == full_text {
                    None
                } else {
                    Some(Tooltip::text(full_text.clone()).into_any_view(cx))
                }
            }),
        )
        .into_any_element()
}

/// The rows the tree draws: the snapshot's items threaded through the `order` arrays and
/// the expansion state, with the unreadable files after them.
fn visible_rows(snapshot: &WorkspaceSnapshot, expanded: &BTreeSet<ItemId>) -> Vec<TreeRow> {
    let mut children_in_order: BTreeMap<ItemId, Vec<ItemId>> = BTreeMap::new();
    let mut has_parent: BTreeSet<ItemId> = BTreeSet::new();
    for (id, item) in &snapshot.items {
        if let Some(order) = container_order(&item.body) {
            children_in_order.insert(*id, order.clone());
            has_parent.extend(order.iter().copied());
        }
    }

    // The roots: every collection — a workspace has no root `order` array to follow, so
    // they sort by name — plus any folder or request no parent's `order` claims. Those
    // orphans are a merge or hand-placement artifact (REQ-COLLAB-02) and drawing them at
    // the root keeps every file on disk reachable (`Decision:`).
    let mut roots: Vec<(String, ItemId)> = snapshot
        .items
        .iter()
        .filter(|(id, item)| {
            matches!(item.body, ItemBody::Collection(_))
                || (matches!(item.body, ItemBody::Folder(_) | ItemBody::Request(_))
                    && !has_parent.contains(id))
        })
        .map(|(id, item)| (item.name(), *id))
        .collect();
    roots.sort_by(|(left_name, left_id), (right_name, right_id)| {
        left_name
            .to_lowercase()
            .cmp(&right_name.to_lowercase())
            .then_with(|| left_name.cmp(right_name))
            .then_with(|| left_id.cmp(right_id))
    });

    let mut rows = Vec::new();
    let mut visited: BTreeSet<ItemId> = BTreeSet::new();
    for (_, id) in roots {
        push_rows(
            snapshot,
            &children_in_order,
            expanded,
            &mut visited,
            id,
            0,
            &mut rows,
        );
    }
    rows.extend(snapshot.problems.iter().map(problem_row));
    rows
}

/// One item's row, and — while it is expanded — its children's, depth first in `order`.
/// `visited` keeps a merge artifact (two parents claiming one child, or a cycle) from
/// drawing a row twice or recursing forever.
fn push_rows(
    snapshot: &WorkspaceSnapshot,
    children_in_order: &BTreeMap<ItemId, Vec<ItemId>>,
    expanded: &BTreeSet<ItemId>,
    visited: &mut BTreeSet<ItemId>,
    id: ItemId,
    depth: usize,
    rows: &mut Vec<TreeRow>,
) {
    if !visited.insert(id) {
        return;
    }
    let Some(item) = snapshot.items.get(&id) else {
        // An `order` entry whose file is gone: there is nothing to draw. The missing
        // file's story belongs to the watcher (S42a).
        return;
    };
    let name = SharedString::from(item.name());
    let is_expanded = expanded.contains(&id);
    match &item.body {
        ItemBody::Collection(_) => rows.push(TreeRow::Collection {
            id,
            name,
            is_expanded,
        }),
        ItemBody::Folder(_) => rows.push(TreeRow::Folder {
            id,
            depth,
            name,
            is_expanded,
        }),
        ItemBody::Request(request) => rows.push(TreeRow::Request {
            id,
            depth,
            name,
            method: SharedString::from(request_kind(request)),
        }),
        // Environments live in the environment picker, and the workspace file is the
        // tree's root rather than a row in it.
        ItemBody::Environment(_) | ItemBody::Workspace(_) => return,
    }
    if is_expanded && let Some(children) = children_in_order.get(&id) {
        for child in children.iter().copied() {
            push_rows(
                snapshot,
                children_in_order,
                expanded,
                visited,
                child,
                depth + 1,
                rows,
            );
        }
    }
}

/// The child ids a container orders, if the item is a container at all.
fn container_order(body: &ItemBody) -> Option<&Vec<ItemId>> {
    match body {
        ItemBody::Collection(collection) => Some(&collection.order),
        ItemBody::Folder(folder) => Some(&folder.order),
        _ => None,
    }
}

/// The method badge text of a request: its HTTP method, or the `type` tag of a request
/// kind this build does not model — a WebSocket or gRPC file still says what it is.
fn request_kind(request: &Request) -> String {
    match request {
        Request::Http(request) => request.method.as_str().to_owned(),
        Request::Unsupported(raw) => raw
            .get("type")
            .and_then(|kind| kind.as_str())
            .unwrap_or("request")
            .to_owned(),
    }
}

/// One unreadable file's row: the file it is about, the short summary its error badge
/// carries, and the problem's own words as the detail (REQ-COLLAB-02).
fn problem_row(problem: &ItemProblem) -> TreeRow {
    let summary = match &problem.kind {
        ItemProblemKind::InvalidJson { .. } => "Invalid JSON",
        ItemProblemKind::ConflictMarkers => "Merge conflict",
        ItemProblemKind::DuplicateId { .. } => "Duplicate id",
    };
    TreeRow::Problem {
        path: SharedString::from(problem.path.to_string_lossy().into_owned()),
        summary: SharedString::new_static(summary),
        detail: SharedString::from(problem.kind.to_string()),
    }
}
