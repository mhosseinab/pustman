//! Drag to reorder and move (S40b): dragging a tree row reorders it among its siblings
//! or moves it into another folder, and the parents' `order` arrays are written through
//! [`workspace_store`] on a background task (REQ-WSP-01). The keyboard's four move keys
//! are the drag's twins, so every move is reachable without a mouse.
//!
//! The writes ride the tree's one write slot — [`crate::crud`]'s queue — and never a
//! second writer: two writes racing on one parent's `order` lose a child from it (S40a
//! fix 1). Like every S40a verb, a move resolves its item, its parents and the target
//! from one fresh read inside the queued task (S40a fix 2), so a rename or delete queued
//! ahead of it cannot leave the move writing stale paths — and one read per write is
//! also all a move scans (S40a's #308 carry-forward, kept in mind).
//!
//! Three doors in:
//! - [`init`] — the move keys, bound against [`COLLECTIONS_TREE_CONTEXT`].
//! - [`CollectionsTree::row_drag`] — one row's drag source and drop target wiring, from
//!   [`crate::tree`]'s row rendering.
//! - [`CollectionsTree`]'s handlers here — the drop and the keys' four moves.
//!
//! `Decision:` collections stay roots. The workspace format has no root `order` array
//! (the tree draws roots in name order), so a collection row accepts drops into it but
//! is never dragged, and a keyboard move on one says so instead of pretending.
//! `Decision:` a move's item file lands in the new parent before either `order` array is
//! written — an `order` must never name a file that is not there (REQ-WSP-01) — and the
//! two parents' arrays are written new-parent first, so a failure part-way leaves the
//! item claimed by the parent that now holds its file.

use std::collections::BTreeSet;
use std::path::Path;

use api_model::ItemId;
use gpui::{
    AnyElement, App, Bounds, Context, Div, DragMoveEvent, IntoElement, Pixels, Point, Render,
    Stateful, Window,
};
use ui::prelude::*;
use workspace_store::{ItemBody, WorkspaceSnapshot};

use crate::crud::{WriteReport, parent_map, remove_child, set_order};
use crate::navigation::COLLECTIONS_TREE_CONTEXT;
use crate::tree::{CollectionsTree, container_order};

#[cfg(test)]
#[path = "reorder_tests.rs"]
mod tests;

/// The height of the insertion line a drop zone draws — the redesign pack's hairline,
/// kept above one pixel so it reads on every scale factor.
const DROP_INDICATOR_PX: f32 = 2.;

gpui::actions!(
    collections,
    [
        /// Moves the selected row before its previous sibling — the keyboard twin of a
        /// drag onto a row's top strip.
        MoveUp,
        /// Moves the selected row after its next sibling.
        MoveDown,
        /// Moves the selected row into the folder directly above it.
        MoveInto,
        /// Moves the selected row out of its folder, landing right after that folder.
        MoveOut,
    ]
);

/// Bind the move keys once per app, against the tree's key context. `alt-` marks a row
/// move wherever the bare arrows only move the selection (the outliner convention).
/// On macOS `alt-left`/`alt-right` are a text field's word moves; the rename field's own
/// deeper key context keeps those, so the row moves answer only while the tree itself
/// holds focus (`.agents/rules/keymaps.md`).
pub fn init(cx: &mut App) {
    cx.bind_keys([
        gpui::KeyBinding::new("alt-up", MoveUp, Some(COLLECTIONS_TREE_CONTEXT)),
        gpui::KeyBinding::new("alt-down", MoveDown, Some(COLLECTIONS_TREE_CONTEXT)),
        gpui::KeyBinding::new("alt-right", MoveInto, Some(COLLECTIONS_TREE_CONTEXT)),
        gpui::KeyBinding::new("alt-left", MoveOut, Some(COLLECTIONS_TREE_CONTEXT)),
    ]);
}

/// What a row drag carries: the item it moves, and the name the drag preview shows.
#[derive(Clone)]
pub(crate) struct RowDrag {
    pub(crate) id: ItemId,
    pub(crate) name: SharedString,
}

/// Where a move places the moved item in the target's parent's `order` array.
#[derive(Clone, Copy)]
pub(crate) enum DropTarget {
    /// Directly before `id`, in the parent that orders `id`.
    Before(ItemId),
    /// Directly after `id`, in the parent that orders `id`.
    After(ItemId),
    /// At the end of container `id`'s `order` — into the container itself.
    Into(ItemId),
}

/// The drop zones a target row offers: an insertion strip at each edge, and — on a
/// container — the middle to drop into.
#[derive(Clone, Copy, PartialEq)]
enum DropZone {
    Before,
    Into,
    After,
}

/// What a row is made of — the zones it offers come from this (a collection has no
/// `order` array to reorder, so it offers only "into").
#[derive(Clone, Copy)]
enum RowKind {
    Collection,
    Folder,
    Request,
}

/// The drop a drag currently hovers: which row, and which of its zones. Exactly one row
/// owns this at a time while a drag is over the tree.
#[derive(Clone, Copy, PartialEq)]
struct DropHover {
    row: ItemId,
    zone: DropZone,
}

/// The drag state one [`CollectionsTree`] carries: where a drop would land while a drag
/// hovers it — the row's drop indicator is drawn from this.
pub(crate) struct Reorder {
    hover: Option<DropHover>,
}

impl Reorder {
    pub(crate) fn new() -> Self {
        Self { hover: None }
    }

    /// Record the hovered drop — true when it changed.
    fn set_hover(&mut self, row: ItemId, zone: DropZone) -> bool {
        let hover = DropHover { row, zone };
        if self.hover == Some(hover) {
            return false;
        }
        self.hover = Some(hover);
        true
    }

    /// Drop `row`'s hover mark — true when it held one (so the row redraws without its
    /// indicator).
    fn clear_hover(&mut self, row: ItemId) -> bool {
        if self.hover.is_some_and(|hover| hover.row == row) {
            self.hover = None;
            return true;
        }
        false
    }

    /// The zone hovered on `row`, if any.
    fn zone_on(&self, row: ItemId) -> Option<DropZone> {
        self.hover
            .filter(|hover| hover.row == row)
            .map(|hover| hover.zone)
    }
}

/// The card that rides the cursor while a row is dragged: the row's name, hung off the
/// point the pointer grabbed it by (the `drag_drop` example's shape).
struct DragPreview {
    name: SharedString,
    grab: Point<Pixels>,
}

impl DragPreview {
    fn new(name: SharedString, grab: Point<Pixels>) -> Self {
        Self { name, grab }
    }
}

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().pl(self.grab.x).pt(self.grab.y).child(
            h_flex()
                .px(DynamicSpacing::Base04.rems(cx))
                .py(DynamicSpacing::Base02.rems(cx))
                .rounded_md()
                .bg(cx.theme().colors().elevated_surface_background)
                .border_1()
                .border_color(cx.theme().colors().border)
                .child(Label::new(self.name.clone()).size(LabelSize::Small)),
        )
    }
}

impl CollectionsTree {
    /// One row's drag wiring: what dragging it carries (the item and its name for the
    /// preview), the drop zones its kind offers, and the indicator that marks the zone
    /// while a drag hovers it. A collection row is a drop target but never a drag
    /// source — it stays a root (`Decision:` above).
    pub(crate) fn row_drag(
        &self,
        frame: Stateful<Div>,
        id: ItemId,
        name: SharedString,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let Some(item) = self.snapshot.items.get(&id) else {
            return frame;
        };
        let kind = match item.body {
            ItemBody::Collection(_) => RowKind::Collection,
            ItemBody::Folder(_) => RowKind::Folder,
            ItemBody::Request(_) => RowKind::Request,
            // Environments and the workspace file draw no row.
            ItemBody::Environment(_) | ItemBody::Workspace(_) => return frame,
        };
        let frame = match kind {
            RowKind::Collection => frame,
            RowKind::Folder | RowKind::Request => frame.on_drag(
                RowDrag { id, name },
                |dragged: &RowDrag, grab, _window, cx| {
                    cx.new(|_cx| DragPreview::new(dragged.name.clone(), grab))
                },
            ),
        };
        // "Into" marks the row itself (the drop target background), while "before" and
        // "after" draw the insertion line through [`Self::drop_indicator`] — an
        // insertion indicator, not just a background (the collections screen recipe).
        let hovered_into = cx.has_active_drag() && self.reorder.zone_on(id) == Some(DropZone::Into);
        frame
            .when(hovered_into, |frame| {
                frame.bg(cx.theme().colors().drop_target_background)
            })
            .on_drag_move::<RowDrag>(cx.listener(
                move |this, event: &DragMoveEvent<RowDrag>, _window, cx| {
                    this.row_drag_hovered(id, kind, event, cx);
                },
            ))
            .on_drop(cx.listener(move |this, dragged: &RowDrag, window, cx| {
                this.row_dropped(id, kind, dragged, window, cx);
            }))
    }

    /// The insertion line for the zone a drag hovers on `id` — drawn over the row's
    /// content, and only while a drag is actually in flight (a hover mark left by a
    /// drag that ended elsewhere must not draw).
    pub(crate) fn drop_indicator(&self, id: ItemId, cx: &App) -> Option<AnyElement> {
        if !cx.has_active_drag() {
            return None;
        }
        let color = cx.theme().colors().drop_target_border;
        let line = |at_top: bool| {
            let line = div()
                .absolute()
                .left_0()
                .right_0()
                .h(px_to_rems(DROP_INDICATOR_PX))
                .bg(color);
            if at_top {
                line.top_0().into_any_element()
            } else {
                line.bottom_0().into_any_element()
            }
        };
        match self.reorder.zone_on(id)? {
            DropZone::Before => Some(line(true)),
            DropZone::After => Some(line(false)),
            // The whole row is the target: its background carries the mark.
            DropZone::Into => None,
        }
    }

    /// `alt-up`: the keyboard twin of dragging the selected row onto its previous
    /// sibling's top strip.
    pub(crate) fn move_up(&mut self, _: &MoveUp, window: &mut Window, cx: &mut Context<Self>) {
        self.keyboard_move(MoveDirection::Up, window, cx);
    }

    /// `alt-down`: the keyboard twin of dragging it onto its next sibling's bottom strip.
    pub(crate) fn move_down(&mut self, _: &MoveDown, window: &mut Window, cx: &mut Context<Self>) {
        self.keyboard_move(MoveDirection::Down, window, cx);
    }

    /// `alt-right`: the keyboard twin of dragging it into the folder above.
    pub(crate) fn move_into(&mut self, _: &MoveInto, window: &mut Window, cx: &mut Context<Self>) {
        self.keyboard_move(MoveDirection::In, window, cx);
    }

    /// `alt-left`: the keyboard twin of dragging it out of its folder — it lands right
    /// after the folder it left.
    pub(crate) fn move_out(&mut self, _: &MoveOut, window: &mut Window, cx: &mut Context<Self>) {
        self.keyboard_move(MoveDirection::Out, window, cx);
    }

    /// The keyboard's four moves, resolved against the snapshot the rows draw and then
    /// written exactly like a drop. A move past the end of the sibling list does nothing
    /// at all — the row visibly stays put, the way the arrows stop at the tree's ends —
    /// while a move with nowhere structurally to go says so (`Decision:`).
    fn keyboard_move(
        &mut self,
        direction: MoveDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected_item() else {
            self.notify_user(
                SharedString::new_static("Select a collection, folder or request first."),
                Severity::Warning,
                window,
                cx,
            );
            return;
        };
        let Some(item) = self.snapshot.items.get(&id) else {
            self.notify_user(
                SharedString::new_static("That item is no longer in this workspace."),
                Severity::Error,
                window,
                cx,
            );
            return;
        };
        if matches!(item.body, ItemBody::Collection(_)) {
            self.notify_user(
                SharedString::new_static(
                    "A collection stays at the top level — only folders and requests can be moved.",
                ),
                Severity::Warning,
                window,
                cx,
            );
            return;
        }
        let name = item.name();
        let parent = parent_map(&self.snapshot.items).get(&id).copied();
        let siblings = parent
            .and_then(|parent| self.snapshot.items.get(&parent))
            .and_then(|item| container_order(&item.body).cloned())
            .unwrap_or_default();
        let position = siblings.iter().position(|sibling| *sibling == id);
        let previous = position
            .and_then(|position| position.checked_sub(1))
            .and_then(|previous| siblings.get(previous).copied());
        let next = position.and_then(|position| siblings.get(position + 1).copied());
        let target = match direction {
            MoveDirection::Up => previous.map(DropTarget::Before),
            MoveDirection::Down => next.map(DropTarget::After),
            MoveDirection::In => previous
                .filter(|previous| {
                    self.snapshot.items.get(previous).is_some_and(|item| {
                        matches!(item.body, ItemBody::Collection(_) | ItemBody::Folder(_))
                    })
                })
                .map(DropTarget::Into),
            MoveDirection::Out => parent
                .filter(|parent| {
                    // Out of a collection has nowhere to go: the roots are not an
                    // `order` array.
                    self.snapshot
                        .items
                        .get(parent)
                        .is_some_and(|item| matches!(item.body, ItemBody::Folder(_)))
                })
                .map(DropTarget::After),
        };
        match target {
            Some(target) => self.queue_move(id, target, window, cx),
            None => self.report_move_dead_end(direction, &name, window, cx),
        }
    }

    /// The two moves with nowhere structurally to go say so — nothing on the row shows
    /// why `alt-right` or `alt-left` did nothing (AGENTS.md › errors reach the user).
    /// The sibling-list ends stay silent: the row visibly cannot move further.
    fn report_move_dead_end(
        &self,
        direction: MoveDirection,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let message = match direction {
            MoveDirection::Up | MoveDirection::Down => return,
            MoveDirection::In => {
                format!("“{name}” has no folder above it to move into.")
            }
            MoveDirection::Out => {
                format!("“{name}” is already at the top level.")
            }
        };
        self.notify_user(message, Severity::Warning, window, cx);
    }

    /// A drag moved: `id` marks itself the drop target while the pointer is over one of
    /// its zones and gives the mark up when it leaves. Every painted row sees every move
    /// of a drag (the listeners are window-wide), so exactly one row owns the mark at a
    /// time — and a row never marks itself (the Zed project panel's shape).
    fn row_drag_hovered(
        &mut self,
        id: ItemId,
        kind: RowKind,
        event: &DragMoveEvent<RowDrag>,
        cx: &mut Context<Self>,
    ) {
        let is_source = event.drag(cx).id == id;
        if is_source {
            if self.reorder.clear_hover(id) {
                cx.notify();
            }
            return;
        }
        if !event.bounds.contains(&event.event.position) {
            if self.reorder.clear_hover(id) {
                cx.notify();
            }
            return;
        }
        let zone = drop_zone(kind, &event.bounds, event.event.position.y);
        if self.reorder.set_hover(id, zone) {
            cx.notify();
        }
    }

    /// The drop: the hovered zone decides where the dragged item lands — before the
    /// row, after it, or into it — and the move goes through the tree's write queue.
    fn row_dropped(
        &mut self,
        id: ItemId,
        kind: RowKind,
        dragged: &RowDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hovered = self.reorder.hover.take();
        if dragged.id == id {
            // A row is not its own destination; the mark goes with the drag either way.
            cx.notify();
            return;
        }
        let zone = hovered
            .filter(|hover| hover.row == id)
            .map_or_else(|| default_zone(kind), |hover| hover.zone);
        let target = match zone {
            DropZone::Before => DropTarget::Before(id),
            DropZone::Into => DropTarget::Into(id),
            DropZone::After => DropTarget::After(id),
        };
        self.queue_move(dragged.id, target, window, cx);
        cx.notify();
    }

    /// Queue one move on the tree's write slot. The answers the snapshot gives are the
    /// cheap ones — does this item exist, is it movable — and the write itself re-reads
    /// everything from disk (S40a fix 2's shape).
    fn queue_move(
        &mut self,
        moved: ItemId,
        target: DropTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let DropTarget::Before(row) | DropTarget::After(row) | DropTarget::Into(row) = target
            && row == moved
        {
            // A row is not its own destination: the drop that means nothing writes nothing.
            return;
        }
        let Some(item) = self.snapshot.items.get(&moved) else {
            self.notify_user(
                SharedString::new_static("That item is no longer in this workspace."),
                Severity::Error,
                window,
                cx,
            );
            return;
        };
        if matches!(item.body, ItemBody::Collection(_)) {
            self.notify_user(
                SharedString::new_static(
                    "A collection stays at the top level — only folders and requests can be moved.",
                ),
                Severity::Warning,
                window,
                cx,
            );
            return;
        }
        let Some(root) = self.writable_root(window, cx) else {
            return;
        };
        let label = item.name();
        // A move into a container opens it, so the moved row is drawn where it landed.
        let expand = match target {
            DropTarget::Into(container) => Some(container),
            DropTarget::Before(_) | DropTarget::After(_) => None,
        };
        self.run_write(expand, window, cx, move || {
            let failure = move_work(&root, moved, target, &label);
            WriteReport::after_write(&root, failure)
        });
    }
}

/// The keyboard's four directions.
#[derive(Clone, Copy)]
enum MoveDirection {
    Up,
    Down,
    In,
    Out,
}

/// One move run — of the items as the disk holds them now: the item, its parents and
/// the target all come from one fresh read inside the queued task (S40a fix 2), so a
/// verb queued ahead of this one cannot leave the move writing stale paths. The item's
/// file moves before the `order` arrays are written (REQ-WSP-01), and every failure
/// says what could not move, where, and why.
fn move_work(root: &Path, moved_id: ItemId, target: DropTarget, label: &str) -> Option<String> {
    let fresh = match workspace_store::open(root) {
        Ok(snapshot) => snapshot,
        Err(error) => return Some(format!("Could not move “{label}”: {error}")),
    };
    let Some(moved) = fresh.items.get(&moved_id).cloned() else {
        return Some(format!(
            "Could not move “{label}”: it is no longer in the workspace"
        ));
    };
    if matches!(moved.body, ItemBody::Collection(_)) {
        return Some(format!(
            "Could not move “{label}”: a collection stays at the top level of a workspace"
        ));
    }
    let name = moved.name();
    let parents = parent_map(&fresh.items);
    let (new_parent_id, anchor) = match target {
        DropTarget::Into(container) => (container, None),
        DropTarget::Before(sibling) | DropTarget::After(sibling) => {
            if sibling == moved_id {
                // A row is not its own destination.
                return None;
            }
            let after = matches!(target, DropTarget::After(_));
            match parents.get(&sibling).copied() {
                Some(parent) => (parent, Some((sibling, after))),
                None => {
                    return Some(format!(
                        "Could not move “{name}”: that row has no parent to move beside"
                    ));
                }
            }
        }
    };
    if new_parent_id == moved_id || descendants(&fresh, moved_id).contains(&new_parent_id) {
        return Some(format!(
            "Could not move “{name}”: it cannot go inside itself"
        ));
    }
    let Some(new_parent) = fresh.items.get(&new_parent_id).cloned() else {
        return Some(format!(
            "Could not move “{name}”: the destination is no longer in the workspace"
        ));
    };
    let Some(new_order) = placed_order(&new_parent.body, moved_id, anchor) else {
        return Some(format!(
            "Could not move “{name}” into “{}”: it cannot hold children",
            new_parent.name()
        ));
    };
    let old_parent_id = parents.get(&moved_id).copied();
    if old_parent_id == Some(new_parent_id) {
        // A reorder inside one parent rewrites one `order` array — and a move that
        // changes nothing writes nothing.
        if container_order(&new_parent.body) == Some(&new_order) {
            return None;
        }
        let mut parent = new_parent;
        set_order(&mut parent.body, new_order);
        return match workspace_store::save_item(root, &parent) {
            Ok(_) => None,
            Err(error) => Some(format!(
                "Could not move “{name}” in “{}”: {error}",
                parent.name()
            )),
        };
    }
    // A cross-parent move takes the item's own file with it first (REQ-WSP-01).
    let Some(new_parent_dir) = new_parent.path.as_deref().and_then(Path::parent) else {
        return Some(format!(
            "Could not move “{name}” into “{}”: it has no file on disk yet",
            new_parent.name()
        ));
    };
    let mut item = moved;
    item.dir = new_parent_dir.to_path_buf();
    if let Err(error) = workspace_store::save_item(root, &item) {
        return Some(format!(
            "Could not move “{name}” into “{}”: {error}",
            new_parent.name()
        ));
    }
    let mut parent = new_parent;
    set_order(&mut parent.body, new_order);
    if let Err(error) = workspace_store::save_item(root, &parent) {
        return Some(format!(
            "Could not add “{name}” to “{}”: {error}",
            parent.name()
        ));
    }
    if let Some(old_parent_id) = old_parent_id
        && let Some(mut old_parent) = fresh.items.get(&old_parent_id).cloned()
    {
        remove_child(&mut old_parent.body, moved_id);
        if let Err(error) = workspace_store::save_item(root, &old_parent) {
            return Some(format!(
                "Could not take “{name}” out of “{}”: {error}",
                old_parent.name()
            ));
        }
    }
    None
}

/// The container's `order` with `moved` placed at `anchor` — before or after the anchor
/// id — or at the end when a drop into the container leaves the place open. [`None`]
/// for an item that cannot hold children at all.
fn placed_order(
    body: &ItemBody,
    moved: ItemId,
    anchor: Option<(ItemId, bool)>,
) -> Option<Vec<ItemId>> {
    let mut order: Vec<ItemId> = container_order(body)?
        .iter()
        .filter(|child| **child != moved)
        .copied()
        .collect();
    match anchor {
        Some((sibling, after)) => {
            let at =
                order
                    .iter()
                    .position(|child| *child == sibling)
                    .map_or(
                        order.len(),
                        |position| {
                            if after { position + 1 } else { position }
                        },
                    );
            order.insert(at, moved);
        }
        None => order.push(moved),
    }
    Some(order)
}

/// Every item inside `id`'s subtree — what a move of `id` must never target, because a
/// drop into one of these would nest a folder inside itself.
fn descendants(snapshot: &WorkspaceSnapshot, id: ItemId) -> BTreeSet<ItemId> {
    let mut descendants = BTreeSet::new();
    let mut frontier = vec![id];
    while let Some(id) = frontier.pop() {
        if let Some(children) = snapshot
            .items
            .get(&id)
            .and_then(|item| container_order(&item.body))
        {
            for child in children {
                if descendants.insert(*child) {
                    frontier.push(*child);
                }
            }
        }
    }
    descendants
}

/// Which zone of a row a pointer at `y` hovers: the insertion strips are the row's top
/// and bottom thirds (halves on a leaf), and a container's middle third drops into it —
/// the three ways a tree row can take a drop.
fn drop_zone(kind: RowKind, bounds: &Bounds<Pixels>, y: Pixels) -> DropZone {
    let height = bounds.size.height.max(px(1.));
    let fraction = (y - bounds.top()) / height;
    match kind {
        RowKind::Collection => DropZone::Into,
        RowKind::Folder if fraction < 1. / 3. => DropZone::Before,
        RowKind::Folder if fraction < 2. / 3. => DropZone::Into,
        RowKind::Folder => DropZone::After,
        RowKind::Request if fraction < 1. / 2. => DropZone::Before,
        RowKind::Request => DropZone::After,
    }
}

/// The zone a drop takes when no hover mark says otherwise — into a container, after a
/// leaf. The drag's own moves normally set the mark long before the drop.
fn default_zone(kind: RowKind) -> DropZone {
    match kind {
        RowKind::Collection | RowKind::Folder => DropZone::Into,
        RowKind::Request => DropZone::After,
    }
}
