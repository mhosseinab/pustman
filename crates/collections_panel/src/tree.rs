//! The Collections panel's tree (PRD REQ-UI-01): the [`WorkspaceSnapshot`]'s collections,
//! folders and requests as a virtualized tree of [`TreeRow`]s, one row per item the
//! snapshot can see.
//!
//! Row order is the `order` arrays' (REQ-WSP-01) — a parent stores its children's ids in
//! display order and the tree walks those, never the name order or the directory order.
//! Containers expand and collapse; every request row carries its method badge. A file the
//! snapshot could not read draws its error badge after the items, with a message naming the
//! file and the line beside it (REQ-COLLAB-02; [`crate::problems`] owns the badge and its
//! details popover).
//!
//! The rows are virtualized through [`VirtualList`] (REQ-NFR-02): the tree flattens the
//! visible rows once per structural change — an expand, a collapse, a new snapshot — and
//! drawing lays out only the rows the window can show.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use api_model::{ItemId, Request};
use gpui::{
    Action, AnyElement, ClickEvent, Context, Div, ElementId, Entity, FocusHandle, Focusable,
    InteractiveText, MouseButton, MouseDownEvent, Render, Role, SharedString, Stateful, Styled,
    StyledText, Subscription, Window,
};
use pustman_actions::import_export::Import;
use pustman_actions::workspace::{CreateWorkspace, OpenWorkspace};
use text_input::{TextInput, TextInputEvent};
use ui::prelude::*;
use workspace::{DockPosition, Panel};
use workspace_store::{ItemBody, WorkspaceSnapshot};

use crate::crud::{Crud, NewCollection};
use crate::navigation::{COLLECTIONS_TREE_CONTEXT, Navigation};
use crate::problems::Problems;
use crate::reorder::Reorder;

#[cfg(test)]
#[path = "tree_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tree_sidebar_tests.rs"]
mod sidebar_tests;

/// The sidebar's controls' stable ids and test selectors (S48u3).
pub(crate) const NEW_COLLECTION_BUTTON_ID: &str = "collections-new";
pub(crate) const FILTER_ID: &str = "collections-filter";
pub(crate) const NO_WORKSPACE_ID: &str = "collections-no-workspace";
pub(crate) const NO_COLLECTIONS_ID: &str = "collections-empty";
pub(crate) const NO_MATCHES_ID: &str = "collections-no-matches";
pub(crate) const EMPTY_NEW_WORKSPACE_ID: &str = "collections-empty-new-workspace";
pub(crate) const EMPTY_OPEN_WORKSPACE_ID: &str = "collections-empty-open-workspace";
pub(crate) const EMPTY_NEW_COLLECTION_ID: &str = "collections-empty-new-collection";
pub(crate) const EMPTY_IMPORT_ID: &str = "collections-empty-import";

/// The selected row's accent bar: the blueprint's 3 px marker at the row's left edge.
const ACCENT_BAR_PX: f32 = 3.;

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
    /// A file the snapshot could not read, with the words its error badge (`summary`),
    /// its tooltip (`detail`) and its details popover (`guidance`) carry.
    Problem {
        path: SharedString,
        summary: SharedString,
        detail: SharedString,
        guidance: SharedString,
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
    /// The snapshot the rows were flattened from — crate-visible, because
    /// [`crate::navigation`] opens the request a selected row names out of it.
    pub(crate) snapshot: WorkspaceSnapshot,
    /// The containers the user has opened. Keyed by [`ItemId`] and kept across snapshot
    /// updates, so an external change never collapses the tree under the pointer (S42a).
    expanded: BTreeSet<ItemId>,
    /// The rows the last flatten produced. Read-only until the next flatten, so drawing
    /// clones the handle instead of re-walking the snapshot.
    rows: Arc<Vec<TreeRow>>,
    /// The keyboard state: the selection, its scroll and the tabs Enter has opened
    /// ([`crate::navigation`]).
    pub(crate) navigation: Navigation,
    /// The CRUD surface: the row menu and the inline rename field ([`crate::crud`]).
    pub(crate) crud: Crud,
    /// The drag state: where a drop lands while a row drag hovers the tree
    /// ([`crate::reorder`]).
    pub(crate) reorder: Reorder,
    /// The problem rows' state: the merge-conflict lines and the one open details
    /// popover ([`crate::problems`]).
    pub(crate) problems: Problems,
    focus_handle: FocusHandle,
    /// "Find in collections" (S48u3): a plain, case-insensitive name filter. Built by
    /// [`Self::new_in_window`] — a text field needs the window — so a tree made without
    /// one simply has no filter field.
    filter: Option<Entity<TextInput>>,
    /// The filter's text as of its last edit, lower-cased and trimmed.
    filter_query: String,
    /// Kept so the filter's edits keep reaching [`Self::on_filter_event`].
    _filter_subscription: Option<Subscription>,
}

impl CollectionsTree {
    /// The tree over `snapshot`. A fresh tree starts collapsed: a workspace can hold
    /// hundreds of collections and the user opens the one they came for.
    pub fn new(snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) -> Self {
        // The panel is one tab stop, so the keyboard can reach the tree (S39b gives it
        // the arrow-key selection on top).
        let focus_handle = cx.focus_handle().tab_stop(true);
        let mut problems = Problems::new(cx);
        problems.refresh(&snapshot.root, &snapshot.problems, cx);
        let rows = Arc::new(visible_rows(&snapshot, &BTreeSet::new(), &problems));
        Self {
            snapshot,
            problems,
            expanded: BTreeSet::new(),
            rows,
            navigation: Navigation::new(),
            crud: Crud::new(),
            reorder: Reorder::new(),
            focus_handle,
            filter: None,
            filter_query: String::new(),
            _filter_subscription: None,
        }
    }

    /// [`Self::new`] with the sidebar's "Find in collections" field — what the app's
    /// panel registration builds.
    pub fn new_in_window(
        snapshot: WorkspaceSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut tree = Self::new(snapshot, cx);
        let filter = cx.new(|cx| {
            TextInput::new(window, cx)
                .with_placeholder(SharedString::new_static("Find in collections"))
        });
        tree._filter_subscription = Some(cx.subscribe(&filter, Self::on_filter_event));
        tree.filter = Some(filter);
        tree
    }

    /// The filter field, when the tree has one.
    pub fn filter(&self) -> Option<&Entity<TextInput>> {
        self.filter.as_ref()
    }

    /// Whether a workspace folder is open: the tree's snapshot before any open has no
    /// root ([`crate::collections_panel`]'s empty snapshot).
    pub fn has_workspace(&self) -> bool {
        !self.snapshot.root.as_os_str().is_empty()
    }

    /// The filter changed: re-derive the rows under the new query.
    fn on_filter_event(
        &mut self,
        filter: Entity<TextInput>,
        event: &TextInputEvent,
        cx: &mut Context<Self>,
    ) {
        if *event != TextInputEvent::Changed {
            return;
        }
        let query = filter.read(cx).text().trim().to_lowercase();
        if query != self.filter_query {
            self.filter_query = query;
            self.reflatten(cx);
        }
    }

    /// The rows the tree draws.
    pub fn rows(&self) -> &[TreeRow] {
        self.rows.as_slice()
    }

    /// Show a different snapshot — the workspace was opened, saved or changed on disk.
    pub fn set_snapshot(&mut self, snapshot: WorkspaceSnapshot, cx: &mut Context<Self>) {
        self.snapshot = snapshot;
        // The rows change under any open details popover, and the new conflicted files
        // need their marker lines read again.
        self.problems
            .refresh(&self.snapshot.root, &self.snapshot.problems, cx);
        self.close_problem_details(cx);
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
        // The rows move under an open details popover.
        self.close_problem_details(cx);
        let is_expanded = !self.expanded.contains(&id);
        self.set_expanded(id, is_expanded, cx);
    }

    /// Re-derive the rows after a structural change (an expand, a collapse, a new
    /// snapshot) and redraw. Never called per frame (`references/performance.md`).
    pub(crate) fn reflatten(&mut self, cx: &mut Context<Self>) {
        self.rows = Arc::new(if self.filter_query.is_empty() {
            visible_rows(&self.snapshot, &self.expanded, &self.problems)
        } else {
            // A filter looks through the whole tree, whatever is collapsed.
            let every_container: BTreeSet<ItemId> = self
                .snapshot
                .items
                .iter()
                .filter(|(_, item)| container_order(&item.body).is_some())
                .map(|(id, _)| *id)
                .collect();
            filter_rows(
                visible_rows(&self.snapshot, &every_container, &self.problems),
                &self.filter_query,
            )
        });
        cx.notify();
    }

    /// One row's content — the wrapper `VirtualList` draws around it carries the row's
    /// stable key and test selector.
    fn render_row(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let row = match self.rows.get(index) {
            Some(row) => row.clone(),
            // The list is told the row count of the same flatten it draws keys from, so
            // an out-of-range index cannot occur — and must not panic if it ever does.
            None => return div().into_any_element(),
        };
        let row_key = row.element_id();
        match row {
            TreeRow::Collection {
                id,
                name,
                is_expanded,
            } => self.render_branch(id, 0, name, is_expanded, &row_key, cx),
            TreeRow::Folder {
                id,
                depth,
                name,
                is_expanded,
            } => self.render_branch(id, depth, name, is_expanded, &row_key, cx),
            TreeRow::Request {
                id,
                depth,
                name,
                method,
            } => self.render_request(id, depth, name, method, &row_key, cx),
            TreeRow::Problem {
                path,
                summary,
                detail,
                guidance,
            } => self.render_problem(path, summary, detail, guidance, &row_key, cx),
        }
    }

    /// The frame every item row shares: one row's height, one indent per depth, and the
    /// columns' fixed widths so the names line up down the tree. The frame is the row's
    /// selectable, clickable surface (the tree-item accessibility node and the selection
    /// highlight live here); the row's stable key and test selector are the wrapper
    /// `VirtualList` draws around it, so this one is that key's `row` child.
    fn row_frame(
        &self,
        depth: usize,
        row_key: &ElementId,
        request: Option<ItemId>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        // The indent follows the nesting depth, clamped explicitly: the depth derives
        // from user data (`order` arrays), so the conversion must never wrap
        // (`.agents/rules/rust-code.md`).
        let indent = INDENT_PX * f32::from(u16::try_from(depth).unwrap_or(u16::MAX));
        let selected = self.navigation.selected_key() == Some(row_key);
        // The selection's test hook, on the frame that draws the highlight.
        let selected_selector = selected.then(|| format!("{row_key}-selected"));
        let click_key = row_key.clone();
        let menu_key = row_key.clone();
        h_flex()
            .id(ElementId::NamedChild(
                Arc::new(row_key.clone()),
                SharedString::new_static("row"),
            ))
            .role(Role::TreeItem)
            .aria_level(depth + 1)
            .aria_selected(selected)
            .w_full()
            .h(px_to_rems(ROW_HEIGHT_PX))
            .min_w_0()
            .items_center()
            .gap(DynamicSpacing::Base01.rems(cx))
            .pl(px_to_rems(indent))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                // A row click takes the keyboard somewhere new, so any details
                // popover goes with it.
                this.close_problem_details(cx);
                this.click_row(click_key.clone(), request, window, cx);
            }))
            // A right-click is this row's action menu, and only this row's: it stops the
            // panel's own menu from opening behind it (S40a).
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.row_context_menu(menu_key.clone(), event.position, window, cx);
                }),
            )
            .relative()
            .when(selected, |row| {
                let accent_selector = format!("{row_key}-accent");
                row.bg(cx.theme().colors().element_selected).child(
                    div()
                        .debug_selector(move || accent_selector)
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px_to_rems(ACCENT_BAR_PX))
                        .rounded_sm()
                        .bg(cx.theme().colors().accent_background),
                )
            })
            .when(!selected, |row| {
                row.hover(|row| row.bg(cx.theme().colors().ghost_element_hover))
            })
            .map(|frame| match selected_selector {
                Some(selector) => frame.debug_selector(move || selector.clone()),
                None => frame,
            })
    }

    /// A collection's or folder's row: the disclosure, the name, and no method badge.
    fn render_branch(
        &self,
        id: ItemId,
        depth: usize,
        name: SharedString,
        is_expanded: bool,
        row_key: &ElementId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.row_frame(depth, row_key, None, cx)
            .aria_expanded(is_expanded)
            // The row is a drag source and a drop target (`crate::reorder`), and the
            // insertion line rides over its content.
            .map(|frame| self.row_drag(frame, id, name.clone(), cx))
            .child(self.disclosure(id, is_expanded, name.clone(), cx))
            .child(self.method_column(row_key, None, cx))
            .child(self.name_element(id, name, row_key, cx))
            .children(self.drop_indicator(id, cx))
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
        row_key: &ElementId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.row_frame(depth, row_key, Some(id), cx)
            .map(|frame| self.row_drag(frame, id, name.clone(), cx))
            .child(div().flex_none().w(px_to_rems(DISCLOSURE_COLUMN_PX)))
            .child(self.method_column(row_key, Some(method), cx))
            .child(self.name_element(id, name, row_key, cx))
            .children(self.drop_indicator(id, cx))
            .into_any_element()
    }

    /// The row's name: the inline rename field while this row is the one being renamed
    /// (S40a), the plain truncated label otherwise.
    fn name_element(
        &self,
        id: ItemId,
        name: SharedString,
        row_key: &ElementId,
        cx: &App,
    ) -> AnyElement {
        if let Some(input) = self.crud.rename_input(id) {
            return div().flex_1().min_w_0().child(input).into_any_element();
        }
        row_label(
            name,
            ElementId::from(format!("{row_key}-name")),
            LabelFont::Ui,
            TextSize::Small,
            Color::Default,
            cx,
        )
    }

    /// A problem row: the error badge (`crate::problems`) beside its message naming the
    /// file and the line — in the same row frame as the items, so the selection walks
    /// over it too.
    fn render_problem(
        &mut self,
        path: SharedString,
        summary: SharedString,
        detail: SharedString,
        guidance: SharedString,
        row_key: &ElementId,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let badge = self.problem_badge(path, summary, detail.clone(), guidance, row_key, cx);
        self.row_frame(0, row_key, None, cx)
            .gap(DynamicSpacing::Base03.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .child(badge)
            .child(row_label(
                detail,
                ElementId::from(format!("{row_key}-name")),
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

    /// The panel's root element: its identity, its key context and the five navigation
    /// handlers. Both render branches start here, so the keyboard answers the same way
    /// over the empty tree as over a full one.
    fn panel_root(&self, panel_element_id: ElementId, cx: &mut Context<Self>) -> Stateful<Div> {
        v_flex()
            .id(panel_element_id)
            .key_context(COLLECTIONS_TREE_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::collapse_or_parent))
            .on_action(cx.listener(Self::expand_or_child))
            .on_action(cx.listener(Self::activate))
            // The CRUD verbs (S40a): the row menu and its creates, and the rename,
            // duplicate and delete the file-manager keys name.
            .on_action(cx.listener(Self::show_row_actions))
            .on_action(cx.listener(Self::new_collection))
            .on_action(cx.listener(Self::new_folder))
            .on_action(cx.listener(Self::new_request))
            .on_action(cx.listener(Self::rename))
            .on_action(cx.listener(Self::duplicate))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::cancel))
            // The move keys (S40b): the keyboard twins of dragging a row around.
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::move_into))
            .on_action(cx.listener(Self::move_out))
            // A right-click off the rows opens the one menu that needs no row: the
            // workspace-level create (S40a).
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.open_row_menu(None, event.position, window, cx);
                }),
            )
            // The open row menu floats above everything the panel draws.
            .children(self.crud.menu_overlay(cx))
            // As does one problem row's details popover.
            .children(self.details_overlay(cx))
    }
}

impl Render for CollectionsTree {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Every element id in this panel derives from its focus handle — the panel's
        // identity — so two panels can never collide in one frame (the shape
        // `command_palette::Picker` builds its ids with).
        let panel_element_id = ElementId::from(&self.focus_handle);
        if !self.has_workspace() {
            return self
                .panel_root(panel_element_id, cx)
                .size_full()
                .font_ui(cx)
                .child(side_empty(
                    NO_WORKSPACE_ID,
                    "Open a workspace to see its collections.",
                    vec![
                        dispatch_button(
                            EMPTY_NEW_WORKSPACE_ID,
                            "New workspace…",
                            Some(IconName::Plus),
                            &CreateWorkspace,
                        )
                        .into_any_element(),
                        dispatch_button(
                            EMPTY_OPEN_WORKSPACE_ID,
                            "Open workspace…",
                            Some(IconName::FolderOpen),
                            &OpenWorkspace,
                        )
                        .into_any_element(),
                    ],
                    cx,
                ))
                .into_any_element();
        }
        if self.rows.is_empty() && self.filter_query.is_empty() {
            return self
                .panel_root(panel_element_id, cx)
                .size_full()
                .font_ui(cx)
                .child(side_empty(
                    NO_COLLECTIONS_ID,
                    "No collections yet. A collection keeps related requests together.",
                    vec![
                        self.create_button(EMPTY_NEW_COLLECTION_ID, "New collection…")
                            .into_any_element(),
                        dispatch_button(
                            EMPTY_IMPORT_ID,
                            "Import…",
                            Some(IconName::Download),
                            &Import,
                        )
                        .into_any_element(),
                    ],
                    cx,
                ))
                .into_any_element();
        }
        let toolbar = self.toolbar(cx);
        if self.rows.is_empty() {
            return self
                .panel_root(panel_element_id, cx)
                .size_full()
                .font_ui(cx)
                .p(DynamicSpacing::Base08.rems(cx))
                .gap(DynamicSpacing::Base04.rems(cx))
                .child(toolbar)
                .child(
                    div().debug_selector(|| NO_MATCHES_ID.to_owned()).child(
                        Label::new("No collection, folder or request matches the filter.")
                            .color(Color::Muted),
                    ),
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
        )
        // The scroll state the selection's moves go through (`navigation`).
        .track_scroll(self.navigation.scroll_handle().clone());
        let list_focused = self.focus_handle.contains_focused(window, cx);

        self.panel_root(panel_element_id, cx)
            .size_full()
            .font_ui(cx)
            .p(DynamicSpacing::Base08.rems(cx))
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(toolbar)
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

    fn create_action(&self, _cx: &App) -> Option<Box<dyn Action>> {
        Some(Box::new(NewCollection))
    }
}

impl CollectionsTree {
    /// The row above the tree: "Find in collections" and "＋ New" (S48u3) — creating a
    /// collection never depends on the right-click menu.
    fn toolbar(&self, cx: &App) -> AnyElement {
        let focus_handle = self.focus_handle.clone();
        h_flex()
            .w_full()
            .flex_none()
            .gap(DynamicSpacing::Base03.rems(cx))
            .children(self.filter.clone().map(|filter| {
                div()
                    .debug_selector(|| FILTER_ID.to_owned())
                    .flex_1()
                    .min_w_0()
                    .child(filter)
            }))
            .when(self.filter.is_none(), |row| row.child(div().flex_1()))
            .child(
                div().debug_selector(|| NEW_COLLECTION_BUTTON_ID.to_owned()).child(
                    Button::new(NEW_COLLECTION_BUTTON_ID, "New")
                        .style(ButtonStyle::Tinted(TintColor::Accent))
                        .start_icon(Icon::new(IconName::Plus))
                        .tooltip(Tooltip::text("New collection"))
                        // Dispatched on the tree's own focus handle, so the tree's
                        // handler answers wherever the keyboard was.
                        .on_click(move |_, window, cx| {
                            focus_handle.dispatch_action(&NewCollection, window, cx);
                        }),
                ),
            )
            .into_any_element()
    }

    /// The empty workspace's New collection… button, with its test selector.
    fn create_button(&self, id: &'static str, label: &'static str) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();
        div().debug_selector(move || id.to_owned()).child(
            Button::new(id, label)
                .style(ButtonStyle::Outlined)
                .full_width()
                .start_icon(Icon::new(IconName::Plus))
                .on_click(move |_, window, cx| {
                    focus_handle.dispatch_action(&NewCollection, window, cx);
                }),
        )
    }
}

/// A sidebar empty state (mockup `.side-empty`): one muted line saying what belongs here,
/// and the buttons that add the first of it, full width under it.
fn side_empty(
    id: &'static str,
    message: &'static str,
    buttons: Vec<AnyElement>,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .debug_selector(move || id.to_owned())
        .w_full()
        .gap(DynamicSpacing::Base04.rems(cx))
        .px(DynamicSpacing::Base08.rems(cx))
        .py(DynamicSpacing::Base12.rems(cx))
        .child(Label::new(message).color(Color::Muted))
        .children(buttons)
}

/// A full-width button that dispatches an app-level `action` (the workspace and import
/// flows answer it on any dispatch path).
fn dispatch_button(
    id: &'static str,
    label: &'static str,
    icon: Option<IconName>,
    action: &dyn Action,
) -> impl IntoElement {
    let action = action.boxed_clone();
    div().debug_selector(move || id.to_owned()).child(
        Button::new(id, label)
            .style(ButtonStyle::Outlined)
            .full_width()
            .map(|button| match icon {
                Some(icon) => button.start_icon(Icon::new(icon)),
                None => button,
            })
            .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx)),
    )
}

/// The rows a filter keeps: every row whose name contains `query` (already lower-cased),
/// and every container above one, so a match is always shown where it lives. Problem
/// rows always stay — an unreadable file is never filtered out of sight.
pub(crate) fn filter_rows(rows: Vec<TreeRow>, query: &str) -> Vec<TreeRow> {
    let mut keep = vec![false; rows.len()];
    // The containers above the current row: (row index, depth).
    let mut ancestors: Vec<(usize, usize)> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let depth = row.depth();
        while ancestors.last().is_some_and(|(_, above)| *above >= depth) {
            ancestors.pop();
        }
        let matches = matches!(row, TreeRow::Problem { .. })
            || row.name().to_lowercase().contains(query);
        if matches {
            if let Some(slot) = keep.get_mut(index) {
                *slot = true;
            }
            for (ancestor, _) in &ancestors {
                if let Some(slot) = keep.get_mut(*ancestor) {
                    *slot = true;
                }
            }
        }
        ancestors.push((index, depth));
    }
    rows.into_iter()
        .zip(keep)
        .filter_map(|(row, kept)| kept.then_some(row))
        .collect()
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
fn visible_rows(
    snapshot: &WorkspaceSnapshot,
    expanded: &BTreeSet<ItemId>,
    problems: &Problems,
) -> Vec<TreeRow> {
    let mut children_in_order: BTreeMap<ItemId, Vec<ItemId>> = BTreeMap::new();
    let mut has_parent: BTreeSet<ItemId> = BTreeSet::new();
    for (id, item) in &snapshot.items {
        if let Some(order) = container_order(&item.body) {
            children_in_order.insert(*id, order.clone());
            has_parent.extend(order.iter().copied());
        }
    }

    let mut rows = Vec::new();
    let mut visited: BTreeSet<ItemId> = BTreeSet::new();

    // The root walk: every collection, once, in name order — a workspace has no root
    // `order` array to follow. A collection is always drawn here and never nested inside
    // another row; see [`push_rows`] for the `order` entries that claim one.
    let collections: Vec<(String, ItemId)> = snapshot
        .items
        .iter()
        .filter(|(_, item)| matches!(item.body, ItemBody::Collection(_)))
        .map(|(id, item)| (item.name(), *id))
        .collect();
    for (_, id) in in_name_order(collections) {
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

    // The orphan roots: every folder or request no root's `order` chain can reach — a
    // file no parent claims, or one only a cycle claims (a rename/rename merge artifact,
    // REQ-COLLAB-02). A root can hold such an item back by being collapsed; an orphan
    // has no root to hold it, so it draws here and every parsed item stays visible
    // exactly once (`Decision:`).
    let reachable = reachable_from_roots(snapshot, &children_in_order, &has_parent);
    let orphans: Vec<(String, ItemId)> = snapshot
        .items
        .iter()
        .filter(|(id, item)| {
            matches!(item.body, ItemBody::Folder(_) | ItemBody::Request(_))
                && !reachable.contains(id)
        })
        .map(|(id, item)| (item.name(), *id))
        .collect();
    for (_, id) in in_name_order(orphans) {
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

    rows.extend(problems.rows(&snapshot.problems));
    rows
}

/// Every item a root's `order` chain can reach — what the tree draws under a root once
/// the user opens it, as opposed to what a collapsed root holds back. Seeded from the
/// collections (always roots) and from any item no `order` claims (nothing to hold it
/// back): only items trapped in a cycle with no entrance stay outside, and those become
/// [`visible_rows`]' orphan roots.
fn reachable_from_roots(
    snapshot: &WorkspaceSnapshot,
    children_in_order: &BTreeMap<ItemId, Vec<ItemId>>,
    has_parent: &BTreeSet<ItemId>,
) -> BTreeSet<ItemId> {
    let mut frontier: Vec<ItemId> = snapshot
        .items
        .iter()
        .filter(|(id, item)| {
            matches!(item.body, ItemBody::Collection(_)) || !has_parent.contains(id)
        })
        .map(|(id, _)| *id)
        .collect();
    let mut reachable = BTreeSet::new();
    while let Some(id) = frontier.pop() {
        if !reachable.insert(id) {
            // Already reached along another `order` chain — and a cycle must not spin.
            continue;
        }
        if let Some(children) = children_in_order.get(&id) {
            frontier.extend(children.iter().copied());
        }
    }
    reachable
}

/// Root rows in their stable order: by name (case-insensitively first), then by id, so
/// two runs over the same snapshot draw the same tree.
fn in_name_order(mut named: Vec<(String, ItemId)>) -> Vec<(String, ItemId)> {
    named.sort_by(|(left_name, left_id), (right_name, right_id)| {
        left_name
            .to_lowercase()
            .cmp(&right_name.to_lowercase())
            .then_with(|| left_name.cmp(right_name))
            .then_with(|| left_id.cmp(right_id))
    });
    named
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
    let Some(item) = snapshot.items.get(&id) else {
        // An `order` entry whose file is gone: there is nothing to draw. The missing
        // file's story belongs to the watcher (S42a).
        return;
    };
    if depth > 0 && matches!(item.body, ItemBody::Collection(_)) {
        // A collection is always a root row, drawn once by the root walk. A merge
        // artifact can claim one inside an `order`, and nesting it here would draw it
        // in the wrong place and twice — so the walk skips it (`Decision:`).
        return;
    }
    if !visited.insert(id) {
        return;
    }
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
pub(crate) fn container_order(body: &ItemBody) -> Option<&Vec<ItemId>> {
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
