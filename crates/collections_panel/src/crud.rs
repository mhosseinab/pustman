//! Collection CRUD (S40a): the context menu and the actions that create, rename (inline),
//! duplicate and delete collections, folders and requests (REQ-UI-03, REQ-WSP-01/03).
//!
//! Every change writes through [`workspace_store::save_item`] — the child's file first,
//! so an `order` array never points at a file that is not there — and the file work runs
//! on a background task (AGENTS.md › no blocking IO on the foreground thread). A failed
//! write becomes an error toast naming the path and the reason (AGENTS.md › errors reach
//! the user), and the tree is refreshed from disk either way, so it draws what the
//! workspace actually holds.
//!
//! Two doors in:
//! - [`init`] — the keys this step binds against [`COLLECTIONS_TREE_CONTEXT`].
//! - [`CollectionsTree`]'s handlers here — the verbs themselves, called by those keys,
//!   by the row's right-click menu and by [`DeleteItemModal`]'s Confirm.
//!
//! `Decision:` the verbs are component actions in this crate's `collections` namespace,
//! bound in code like [`crate::navigation`]'s, not `pustman_actions` entries:
//! `pustman_actions` is PRD command-palette surface (its keymap lives in
//! `assets/keymaps/`), and this step may not edit either. They are reachable without a
//! mouse all the same — every verb is on the row's action menu (right-click or
//! Shift-F10) and the menu is keyboard-navigable — and the four file-manager verbs also
//! have direct keys (`f2`, `delete`, `ctrl-d`/`cmd-d`, the `n` family).

use std::collections::{BTreeMap, VecDeque};
use std::path::{Component, Path, PathBuf};

use api_model::{Auth, Body, Collection, Folder, HttpRequest, ItemId, Method, Request};
use gpui::{
    Anchor, App, Context, DismissEvent, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    Pixels, Point, Subscription, Task, WeakEntity, Window, anchored, deferred, div,
};
use text_input::input::SelectAll;
use text_input::{TextInput, TextInputEvent};
use ui::components::context_menu::ContextMenuEvent;
use ui::prelude::*;
use workspace::{ModalView, Workspace};
use workspace_store::{Item, ItemBody, WorkspaceSnapshot};

use crate::navigation::COLLECTIONS_TREE_CONTEXT;
use crate::tree::{CollectionsTree, TreeRow, container_order};

#[cfg(test)]
#[path = "crud_tests.rs"]
mod tests;

/// The directory top-level collections live in (REQ-WSP-01): `workspace_store` reads
/// and writes every collection and folder under it.
const COLLECTIONS_DIR: &str = "collections";

/// What a freshly created item is named before the user renames it — the name Postman
/// gives the same three creates, so the tree and the file both say what it is.
const NEW_COLLECTION_NAME: &str = "New Collection";
const NEW_FOLDER_NAME: &str = "New Folder";
const NEW_REQUEST_NAME: &str = "New Request";

/// The row menu's entries, as they read in the menu.
const NEW_COLLECTION_ENTRY: &str = "New Collection";
const NEW_FOLDER_ENTRY: &str = "New Folder";
const NEW_REQUEST_ENTRY: &str = "New Request";
const RENAME_ENTRY: &str = "Rename";
const DUPLICATE_ENTRY: &str = "Duplicate";
const DELETE_ENTRY: &str = "Delete";

/// The delete confirm modal's Cancel button test hook (the history panel's modal's
/// shape): a stable selector beside the element id.
pub(crate) const DELETE_CANCEL_BUTTON_SELECTOR: &str = "crud-delete-cancel";

/// The confirm button's test hook.
pub(crate) const DELETE_CONFIRM_BUTTON_SELECTOR: &str = "crud-delete-confirm";

gpui::actions!(
    collections,
    [
        /// Opens the row's action menu — `Shift-F10`, the keyboard twin of a right-click.
        ShowRowActions,
        /// Creates a collection at the workspace root.
        NewCollection,
        /// Creates a folder in the selected collection or folder.
        NewFolder,
        /// Creates a request in the selected collection or folder.
        NewRequest,
        /// Renames the selected item in place.
        Rename,
        /// Duplicates the selected item and everything inside it.
        Duplicate,
        /// Deletes the selected item, behind a confirm dialog.
        Delete,
    ]
);

/// Bind the verbs once per app, against the tree's key context. The `n` family follows
/// the platform's own "new" modifier (`cmd-` on macOS, `ctrl-` elsewhere), the same
/// split `text_input::init` draws for its editing keys.
pub fn init(cx: &mut App) {
    let (duplicate, new_request, new_folder, new_collection) = if cfg!(target_os = "macos") {
        ("cmd-d", "cmd-n", "cmd-shift-n", "cmd-alt-n")
    } else {
        ("ctrl-d", "ctrl-n", "ctrl-shift-n", "ctrl-alt-n")
    };
    cx.bind_keys([
        gpui::KeyBinding::new("shift-f10", ShowRowActions, Some(COLLECTIONS_TREE_CONTEXT)),
        gpui::KeyBinding::new("f2", Rename, Some(COLLECTIONS_TREE_CONTEXT)),
        gpui::KeyBinding::new("delete", Delete, Some(COLLECTIONS_TREE_CONTEXT)),
        gpui::KeyBinding::new(duplicate, Duplicate, Some(COLLECTIONS_TREE_CONTEXT)),
        gpui::KeyBinding::new(new_request, NewRequest, Some(COLLECTIONS_TREE_CONTEXT)),
        gpui::KeyBinding::new(new_folder, NewFolder, Some(COLLECTIONS_TREE_CONTEXT)),
        gpui::KeyBinding::new(
            new_collection,
            NewCollection,
            Some(COLLECTIONS_TREE_CONTEXT),
        ),
        // Escape backs out of the inline rename without writing it.
        gpui::KeyBinding::new("escape", Cancel, Some(COLLECTIONS_TREE_CONTEXT)),
    ]);
}

/// The CRUD state one [`CollectionsTree`] carries: the row menu on screen (with the
/// subscription that says when it is done), the inline rename field — one at a time,
/// like the [`ContextMenu`] itself — and the tree's one write slot (S40a fix 1).
pub(crate) struct Crud {
    row_menu: Option<Entity<ContextMenu>>,
    row_menu_subscription: Option<Subscription>,
    /// Where the menu opens: the pointer, in window coordinates ([`gpui::anchored`]'s
    /// own frame).
    menu_position: Point<Pixels>,
    renaming: Option<RenameState>,
    /// The one write in flight. Further verbs wait in [`Self::queued_writes`] until it
    /// finishes: two writes racing on one parent's `order` lose a child from it.
    pending_write: Option<Task<()>>,
    queued_writes: VecDeque<QueuedWrite>,
}

/// The one row being renamed inline, with the field doing it and its subscriptions.
struct RenameState {
    id: ItemId,
    input: Entity<TextInput>,
    // Held so the Submitted and blur events keep arriving; dropping them detaches.
    _subscriptions: [Subscription; 2],
}

/// A verb waiting its turn on the tree's one write slot.
struct QueuedWrite {
    /// The container to expand once the write lands.
    expand: Option<ItemId>,
    /// Where a failure toast goes when the write lands.
    workspace: Option<WeakEntity<Workspace>>,
    /// The write itself, run on a background task.
    work: Box<dyn FnOnce() -> WriteReport + Send + 'static>,
}

impl Crud {
    pub(crate) fn new() -> Self {
        Self {
            row_menu: None,
            row_menu_subscription: None,
            menu_position: Point::default(),
            renaming: None,
            pending_write: None,
            queued_writes: VecDeque::new(),
        }
    }

    /// The open row menu as a floating overlay at [`Self::menu_position`], or nothing
    /// while no menu is open.
    pub(crate) fn menu_overlay(&self, cx: &App) -> Option<gpui::AnyElement> {
        let menu = self.row_menu.clone()?;
        let position = self.menu_position;
        Some(
            div()
                .absolute()
                .top(px(0.))
                .left_0()
                .child(
                    deferred(
                        anchored()
                            .anchor(Anchor::TopLeft)
                            .position(position)
                            .snap_to_window_with_margin(DynamicSpacing::Base04.px(cx))
                            .child(menu),
                    )
                    .with_priority(1),
                )
                .into_any_element(),
        )
    }

    /// The rename field for `id`, when that row is the one being renamed inline.
    pub(crate) fn rename_input(&self, id: ItemId) -> Option<Entity<TextInput>> {
        self.renaming
            .as_ref()
            .filter(|state| state.id == id)
            .map(|state| state.input.clone())
    }
}

/// Which kind of child a create adds under a container.
#[derive(Clone, Copy)]
pub(crate) enum ChildKind {
    Folder,
    Request,
}

impl CollectionsTree {
    /// `Shift-F10`: the selected row's action menu at the pointer — the keyboard twin of
    /// the row's right-click, so every verb is reachable without a mouse. With nothing
    /// selected it offers the one verb that needs no target: a new collection.
    pub(crate) fn show_row_actions(
        &mut self,
        _: &ShowRowActions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row = self.selected_key();
        self.open_row_menu(row, window.mouse_position(), window, cx);
    }

    /// A right-click on a row: the same menu, at the pointer (the click's own position —
    /// `MouseDownEvent` and `gpui::anchored` speak the same window coordinates).
    pub(crate) fn row_context_menu(
        &mut self,
        row: ElementId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_row_menu(Some(row), position, window, cx);
    }

    /// The action menu for `row` (or for the panel itself when there is no row), built
    /// for what that row is: the creates that need a container, then the item's own
    /// rename, duplicate and delete (a file explorer's menu shape).
    pub(crate) fn open_row_menu(
        &mut self,
        row: Option<ElementId>,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = row.as_ref().and_then(|key| self.item_at(key));
        if row.is_some() && item.is_none() {
            // A problem row names a file, not an item: there is nothing to act on.
            return;
        }
        let container = self.creation_parent(item);
        let tree = cx.weak_entity();
        let menu = cx.new(|cx| {
            let mut menu = ContextMenu::new(cx).entry(NEW_COLLECTION_ENTRY, {
                let tree = tree.clone();
                move |_event, window, cx| {
                    if tree
                        .update(cx, |tree, cx| tree.create_collection(window, cx))
                        .is_err()
                    {
                        // Handled: the panel went away while the menu was open; there is
                        // nothing left to create into.
                    }
                }
            });
            if item.is_some() {
                menu = menu.separator();
                match container {
                    Some(container) => {
                        menu = menu
                            .entry(NEW_FOLDER_ENTRY, {
                                let tree = tree.clone();
                                move |_event, window, cx| {
                                    if tree
                                        .update(cx, |tree, cx| {
                                            tree.create_child(
                                                Some(container),
                                                ChildKind::Folder,
                                                window,
                                                cx,
                                            );
                                        })
                                        .is_err()
                                    {
                                        // Handled: the panel went away while the menu was open.
                                    }
                                }
                            })
                            .entry(NEW_REQUEST_ENTRY, {
                                let tree = tree.clone();
                                move |_event, window, cx| {
                                    if tree
                                        .update(cx, |tree, cx| {
                                            tree.create_child(
                                                Some(container),
                                                ChildKind::Request,
                                                window,
                                                cx,
                                            );
                                        })
                                        .is_err()
                                    {
                                        // Handled: the panel went away while the menu was open.
                                    }
                                }
                            });
                    }
                    None => {
                        // An item no container claims has nowhere to put children; the
                        // entries are shown but cannot run.
                        menu = menu
                            .disabled_entry(NEW_FOLDER_ENTRY)
                            .disabled_entry(NEW_REQUEST_ENTRY);
                    }
                }
                if let Some(id) = item {
                    menu = menu
                        .separator()
                        .entry(RENAME_ENTRY, {
                            let tree = tree.clone();
                            move |_event, window, cx| {
                                if tree
                                    .update(cx, |tree, cx| tree.begin_rename(id, window, cx))
                                    .is_err()
                                {
                                    // Handled: the panel went away while the menu was open.
                                }
                            }
                        })
                        .entry(DUPLICATE_ENTRY, {
                            let tree = tree.clone();
                            move |_event, window, cx| {
                                if tree
                                    .update(cx, |tree, cx| tree.duplicate_item(id, window, cx))
                                    .is_err()
                                {
                                    // Handled: the panel went away while the menu was open.
                                }
                            }
                        })
                        .colored_entry(DELETE_ENTRY, Color::Error, {
                            let tree = tree.clone();
                            move |_event, window, cx| {
                                if tree
                                    .update(cx, |tree, cx| tree.begin_delete(id, window, cx))
                                    .is_err()
                                {
                                    // Handled: the panel went away while the menu was open.
                                }
                            }
                        });
                }
            }
            menu
        });
        let subscription = cx.subscribe_in(&menu, window, Self::on_row_menu_event);
        menu.update(cx, |menu, cx| menu.focus(window, cx));
        self.crud.row_menu = Some(menu);
        self.crud.row_menu_subscription = Some(subscription);
        self.crud.menu_position = position;
        cx.notify();
    }

    /// The menu is done — an entry ran or Escape backed out — so it goes away.
    fn on_row_menu_event(
        &mut self,
        _menu: &Entity<ContextMenu>,
        event: &ContextMenuEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != ContextMenuEvent::Dismissed {
            return;
        }
        self.crud.row_menu = None;
        self.crud.row_menu_subscription = None;
        cx.notify();
    }

    /// The keyboard's New Collection: the one create with no parent (REQ-UI-03).
    pub(crate) fn new_collection(
        &mut self,
        _: &NewCollection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.create_collection(window, cx);
    }

    /// The keyboard's New Folder, under the selected container (or the selected item's).
    pub(crate) fn new_folder(
        &mut self,
        _: &NewFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.create_child(None, ChildKind::Folder, window, cx);
    }

    /// The keyboard's New Request, under the selected container (or the selected item's).
    pub(crate) fn new_request(
        &mut self,
        _: &NewRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.create_child(None, ChildKind::Request, window, cx);
    }

    /// `F2`: rename the selected item in place.
    pub(crate) fn rename(&mut self, _: &Rename, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_item() {
            self.begin_rename(id, window, cx);
        } else {
            self.report_no_selection(window, cx);
        }
    }

    /// Duplicate the selected item.
    pub(crate) fn duplicate(&mut self, _: &Duplicate, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_item() {
            self.duplicate_item(id, window, cx);
        } else {
            self.report_no_selection(window, cx);
        }
    }

    /// `Delete`: the selected item goes behind the confirm modal — never straight out.
    pub(crate) fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.selected_item() {
            self.begin_delete(id, window, cx);
        } else {
            self.report_no_selection(window, cx);
        }
    }

    /// Escape: back out of the inline rename without writing it.
    pub(crate) fn cancel(&mut self, _: &Cancel, _window: &mut Window, cx: &mut Context<Self>) {
        if self.crud.renaming.take().is_some() {
            cx.notify();
        }
    }

    /// The user tried a verb that needs a selected row and nothing is selected.
    fn report_no_selection(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.notify_user(
            SharedString::new_static("Select a collection, folder or request first."),
            Severity::Warning,
            window,
            cx,
        );
    }

    /// Create a collection at the workspace root: the file goes in `collections/`
    /// (REQ-WSP-01) and there is no parent's `order` to update.
    fn create_collection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.writable_root(window, cx) else {
            return;
        };
        let id = ItemId::new();
        let item = Item {
            body: ItemBody::Collection(Collection {
                id,
                name: String::from(NEW_COLLECTION_NAME),
                description: None,
                auth: Auth::Inherit,
                variables: Vec::new(),
                order: Vec::new(),
                children: Vec::new(),
                extra: BTreeMap::new(),
            }),
            dir: PathBuf::from(COLLECTIONS_DIR),
            path: None,
        };
        let name = item.name();
        self.run_write(None, window, cx, move || {
            let failure = match workspace_store::save_item(&root, &item) {
                Ok(_) => None,
                Err(error) => Some(format!(
                    "Could not create “{name}” in {COLLECTIONS_DIR}/: {error}"
                )),
            };
            WriteReport::after_write(&root, failure)
        });
    }

    /// Create a folder or request under `container` — the selected container, or the
    /// selected item's own container (the file-explorer convention) — and add it to that
    /// container's `order` (REQ-WSP-01).
    pub(crate) fn create_child(
        &mut self,
        container: Option<ItemId>,
        kind: ChildKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.writable_root(window, cx) else {
            return;
        };
        let Some(container) = container.or_else(|| self.creation_parent(self.selected_item()))
        else {
            self.notify_user(
                SharedString::new_static("Select a collection or folder to add to first."),
                Severity::Warning,
                window,
                cx,
            );
            return;
        };
        let Some(parent) = self.snapshot.items.get(&container).cloned() else {
            self.notify_user(
                SharedString::new_static(
                    "That collection or folder is no longer in this workspace.",
                ),
                Severity::Error,
                window,
                cx,
            );
            return;
        };
        // A child lives in its container's own `<slug>` directory — the directory the
        // container's file lives in (REQ-WSP-01).
        let Some(parent_dir) = parent
            .path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
        else {
            self.notify_user(
                format!(
                    "“{}” has no file on disk yet, so nothing can be added under it.",
                    parent.name()
                ),
                Severity::Error,
                window,
                cx,
            );
            return;
        };
        let id = ItemId::new();
        let item = Item {
            body: child_body(kind, id),
            dir: parent_dir,
            path: None,
        };
        let name = item.name();
        let parent_name = parent.name();
        self.run_write(Some(container), window, cx, move || {
            // Everything from the disk as it holds it NOW (S40a fix 2): a create queued
            // behind a delete must not resurrect what the delete removed, and a parent
            // that is gone means writing nothing at all.
            let parent_now = match current_item(&root, container, &parent_name) {
                Ok(parent_now) => parent_now,
                Err(failure) => return WriteReport::after_write(&root, Some(failure)),
            };
            let Some(mut parent) = parent_now else {
                return WriteReport::after_write(
                    &root,
                    Some(format!(
                        "Could not create “{name}” in “{parent_name}”: it is no longer in the workspace"
                    )),
                );
            };
            let mut item = item;
            if let Some(dir) = parent.path.as_deref().and_then(Path::parent) {
                item.dir = dir.to_path_buf();
            }
            // The child's file first: an `order` must never name a file that is not
            // there (REQ-WSP-01).
            let mut failure = match workspace_store::save_item(&root, &item) {
                Ok(_) => None,
                Err(error) => Some(format!(
                    "Could not create “{name}” in “{parent_name}”: {error}"
                )),
            };
            if failure.is_none() {
                append_child(&mut parent.body, id);
                if let Err(error) = workspace_store::save_item(&root, &parent) {
                    failure = Some(format!(
                        "Could not add “{name}” to “{parent_name}”: {error}"
                    ));
                }
            }
            WriteReport::after_write(&root, failure)
        });
    }

    /// `F2`'s work: the row's name becomes a field holding the old name, with the name
    /// selected so typing replaces it (the file-manager rename gesture). Enter writes it
    /// through [`Self::commit_rename`]; Escape drops it.
    fn begin_rename(&mut self, id: ItemId, window: &mut Window, cx: &mut Context<Self>) {
        if self.writable_root(window, cx).is_none() {
            return;
        }
        let Some(name) = self.snapshot.items.get(&id).map(Item::name) else {
            self.notify_user(
                SharedString::new_static("That item is no longer in this workspace."),
                Severity::Error,
                window,
                cx,
            );
            return;
        };
        let input = cx.new(|cx| TextInput::new(window, cx));
        input.update(cx, |input, cx| input.set_text(&name, cx));
        let focus_handle = input.read(cx).focus_handle(cx);
        window.focus(&focus_handle, cx);
        // Select-all rides the next frame, after the field has been drawn once: the
        // action dispatches along the focused element's *rendered* path, and the field
        // is not in last frame's tree yet.
        window.on_next_frame(|window, cx| {
            window.dispatch_action(Box::new(SelectAll), cx);
        });
        let subscription = cx.subscribe_in(&input, window, Self::on_rename_event);
        // Losing focus abandons the rename without writing it (S40a fix 4): the field
        // closing on blur is what a click-away means everywhere else in the app.
        let blur_subscription = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            if this.crud.renaming.take().is_some() {
                cx.notify();
            }
        });
        self.crud.renaming = Some(RenameState {
            id,
            input,
            _subscriptions: [subscription, blur_subscription],
        });
        cx.notify();
    }

    /// The rename field submitted: the typed name is the rename to write.
    fn on_rename_event(
        &mut self,
        input: &Entity<TextInput>,
        event: &TextInputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if *event != TextInputEvent::Submitted {
            return;
        }
        let Some(state) = self.crud.renaming.take() else {
            return;
        };
        let name = input.read(cx).text();
        cx.notify();
        self.commit_rename(state.id, name, window, cx);
    }

    /// Write the rename: the `id` inside the file never changes and the file follows the
    /// name's slug (REQ-WSP-03). An empty or unchanged name writes nothing — an empty
    /// name is not a name, and an unchanged one is the zero-byte diff of a no-op
    /// (`Decision:`).
    pub(crate) fn commit_rename(
        &mut self,
        id: ItemId,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = name.trim().to_owned();
        let Some(item) = self.snapshot.items.get(&id).cloned() else {
            self.notify_user(
                SharedString::new_static("That item is no longer in this workspace."),
                Severity::Error,
                window,
                cx,
            );
            return;
        };
        if name.is_empty() || name == item.name() {
            return;
        }
        let Some(root) = self.writable_root(window, cx) else {
            return;
        };
        let old_name = item.name();
        let old_path = item.path.clone().unwrap_or_default();
        self.run_write(None, window, cx, move || {
            // The item as the disk holds it now (S40a fix 1), renamed in place:
            // `save_item` keeps the `id` and moves the file (or the whole `<slug>`
            // directory) to the new slug, taking the old file with it (REQ-WSP-03).
            let failure = match current_item(&root, id, &old_name) {
                Ok(Some(mut renamed)) => {
                    renamed.set_name(&name);
                    match workspace_store::save_item(&root, &renamed) {
                        Ok(_) => None,
                        Err(error) => Some(format!(
                            "Could not rename “{old_name}” ({}): {error}",
                            old_path.display()
                        )),
                    }
                }
                Ok(None) => Some(format!(
                    "Could not rename “{old_name}” ({}): it is no longer in the workspace",
                    old_path.display()
                )),
                Err(failure) => Some(failure),
            };
            WriteReport::after_write(&root, failure)
        });
    }

    /// Duplicate `id` and — for a folder or collection — everything inside it, under
    /// "Copy of …" with fresh ids, beside the original.
    pub(crate) fn duplicate_item(
        &mut self,
        id: ItemId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.writable_root(window, cx) else {
            return;
        };
        let Some(source) = self.snapshot.items.get(&id).cloned() else {
            self.notify_user(
                SharedString::new_static("That item is no longer in this workspace."),
                Severity::Error,
                window,
                cx,
            );
            return;
        };
        let label = source.name();
        self.run_write(None, window, cx, move || {
            // The source subtree comes from the disk as it holds it NOW (S40a fix 2):
            // a rename queued ahead of this verb can have moved it and its whole
            // directory, and a copy built from the queued snapshot would land outside
            // its container — recreating the directory the rename left behind.
            let failure = duplicate_work(&root, id, &label);
            WriteReport::after_write(&root, failure)
        });
    }

    /// `Delete`'s first half: the confirm modal (PRD §3's destructive-confirm rule).
    /// Nothing is removed until [`DeleteItemModal`] confirms.
    fn begin_delete(&mut self, id: ItemId, window: &mut Window, cx: &mut Context<Self>) {
        if self.writable_root(window, cx).is_none() {
            return;
        }
        let Some(item) = self.snapshot.items.get(&id) else {
            self.notify_user(
                SharedString::new_static("That item is no longer in this workspace."),
                Severity::Error,
                window,
                cx,
            );
            return;
        };
        let name = SharedString::from(item.name());
        let is_container = matches!(item.body, ItemBody::Collection(_) | ItemBody::Folder(_));
        let Some(Some(workspace)) = window.root::<Workspace>() else {
            // The panel draws inside a workspace window; without one there is no modal
            // layer to confirm in.
            return;
        };
        let tree = cx.weak_entity();
        workspace.update(cx, |workspace, cx| {
            workspace.toggle_modal(window, cx, |_, cx| {
                DeleteItemModal::new(tree.clone(), id, name.clone(), is_container, cx)
            });
        });
    }

    /// The confirm modal's Confirm: the only path that removes anything. The item leaves
    /// its parent's `order` first (REQ-WSP-01), then its file(s) go (REQ-WSP-01's one
    /// file per item: a container takes its `<slug>` directory and everything in it).
    pub(crate) fn confirm_delete(
        &mut self,
        id: ItemId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.writable_root(window, cx) else {
            return;
        };
        let Some(item) = self.snapshot.items.get(&id) else {
            return;
        };
        let label = item.name();
        self.run_write(None, window, cx, move || {
            let failure = delete_work(&root, id, &label);
            WriteReport::after_write(&root, failure)
        });
    }

    /// The container a create lands in when it is aimed at `around`: the row itself when
    /// it is a collection or folder, its parent when it is a request, and nothing at all
    /// for an item no container claims.
    fn creation_parent(&self, around: Option<ItemId>) -> Option<ItemId> {
        let id = around?;
        match self.snapshot.items.get(&id).map(|item| &item.body) {
            Some(ItemBody::Collection(_)) | Some(ItemBody::Folder(_)) => Some(id),
            _ => parent_map(&self.snapshot.items).get(&id).copied(),
        }
    }

    /// The selected row's item — [`None`] with no selection and on a problem row.
    pub(crate) fn selected_item(&self) -> Option<ItemId> {
        self.selected_key().and_then(|key| self.item_at(&key))
    }

    /// The selected row's stable key.
    fn selected_key(&self) -> Option<ElementId> {
        self.navigation.selected_key().cloned()
    }

    /// The item the row `key` draws, if it draws one.
    fn item_at(&self, key: &ElementId) -> Option<ItemId> {
        let index = self.navigation.index_of(key, self.rows())?;
        row_item_id(self.rows().get(index)?)
    }

    /// The workspace root, if this workspace takes writes at all. A read-only workspace
    /// (REQ-WSP-04) and an empty panel state (no workspace open) are reported, not
    /// silently ignored.
    pub(crate) fn writable_root(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<PathBuf> {
        if self.snapshot.read_only {
            self.notify_user(
                SharedString::new_static(
                    "This workspace is read-only: it declares a newer format version than this build writes.",
                ),
                Severity::Error,
                window,
                cx,
            );
            return None;
        }
        if self.snapshot.root.as_os_str().is_empty() {
            self.notify_user(
                SharedString::new_static(
                    "No workspace is open — open one to change its collections.",
                ),
                Severity::Error,
                window,
                cx,
            );
            return None;
        }
        Some(self.snapshot.root.clone())
    }

    /// Put `message` in front of the user — a toast on the workspace window, or the log
    /// when that window is already gone (S40c's shape: path and reason only, never
    /// request content).
    pub(crate) fn notify_user(
        &self,
        message: impl Into<SharedString>,
        severity: Severity,
        window: &mut Window,
        cx: &mut App,
    ) {
        let message = message.into();
        if let Some(Some(workspace)) = window.root::<Workspace>() {
            workspace.update(cx, |workspace, cx| {
                workspace.show_toast(message, severity, None, cx);
            });
            return;
        }
        log::error!("{message}");
    }

    /// Queue one write on the tree's single write slot and start it if the slot is
    /// free (AGENTS.md › no blocking IO on the foreground thread; S40a fix 1). Every
    /// verb — create, rename, duplicate, delete, and [`crate::reorder`]'s moves — lands
    /// through here.
    pub(crate) fn run_write(
        &mut self,
        expand: Option<ItemId>,
        window: &mut Window,
        cx: &mut Context<Self>,
        work: impl FnOnce() -> WriteReport + Send + 'static,
    ) {
        let workspace = window
            .root::<Workspace>()
            .and_then(|workspace| workspace)
            .map(|workspace| workspace.downgrade());
        self.crud.queued_writes.push_back(QueuedWrite {
            expand,
            workspace,
            work: Box::new(work),
        });
        self.start_next_write(cx);
    }

    /// Start the oldest queued write unless one is already running. One write at a time
    /// per tree: two writes racing on the same parent's `order` lose a child from it,
    /// and snapshots then land out of order. The next write starts only once the
    /// previous one's snapshot is applied, so snapshots apply in completion order and
    /// each write's parent re-read sees everything before it (S40a fix 1).
    fn start_next_write(&mut self, cx: &mut Context<Self>) {
        if self.crud.pending_write.is_some() {
            return;
        }
        let Some(next) = self.crud.queued_writes.pop_front() else {
            return;
        };
        self.crud.pending_write = Some(cx.spawn(async move |this, cx| {
            let report = cx.background_spawn(async move { (next.work)() }).await;
            let failure = report
                .failure
                .or_else(|| report.refreshed.as_ref().err().cloned());
            let landed = this.update(cx, |tree, cx| {
                if let Ok(snapshot) = report.refreshed {
                    tree.set_snapshot(snapshot, cx);
                    if let Some(container) = next.expand {
                        // The new row is in there: open the container so it is drawn.
                        tree.set_expanded(container, true, cx);
                    }
                }
            });
            if landed.is_err() {
                // Handled: the panel went away with its window before the write landed;
                // the workspace below still hears about a failure.
            }
            if let Some(message) = failure {
                let mut shown = false;
                if let Some(workspace) = &next.workspace {
                    shown = workspace
                        .update(cx, |workspace, cx| {
                            workspace.show_toast(message.clone(), Severity::Error, None, cx);
                        })
                        .is_ok();
                }
                if !shown {
                    log::error!("{message}");
                }
            }
            // The write ran to completion (its work landed before this point, so
            // nothing is cancelled by handing the slot on) — the next queued verb
            // goes now.
            if this
                .update(cx, |tree, cx| {
                    tree.crud.pending_write = None;
                    tree.start_next_write(cx);
                })
                .is_err()
            {
                // Handled: the panel went away with its window; its queue goes too.
            }
        }));
    }
}

/// What a background write reports back: the workspace as it now stands on disk, and
/// what to tell the user when the write itself failed.
pub(crate) struct WriteReport {
    /// Read after the write — a write that failed part-way can still have changed files.
    refreshed: Result<WorkspaceSnapshot, String>,
    failure: Option<String>,
}

impl WriteReport {
    /// The report after `work` ran: the re-read workspace plus `failure`, unchanged.
    pub(crate) fn after_write(root: &Path, failure: Option<String>) -> Self {
        Self {
            refreshed: workspace_store::open(root).map_err(|error| {
                format!(
                    "Could not read {} back after the change: {error}",
                    root.display()
                )
            }),
            failure,
        }
    }
}

/// The body a fresh child is written as: a folder with no children yet, or a GET to an
/// empty URL — the same blank draft `request_editor` opens in a new tab.
fn child_body(kind: ChildKind, id: ItemId) -> ItemBody {
    match kind {
        ChildKind::Folder => ItemBody::Folder(Folder {
            id,
            name: String::from(NEW_FOLDER_NAME),
            description: None,
            auth: Auth::Inherit,
            children: Vec::new(),
            order: Vec::new(),
            extra: BTreeMap::new(),
        }),
        ChildKind::Request => ItemBody::Request(Request::Http(HttpRequest {
            id,
            name: String::from(NEW_REQUEST_NAME),
            method: Method::Get,
            url: String::new(),
            params: Vec::new(),
            path_variables: Vec::new(),
            headers: Vec::new(),
            body: Body::None,
            auth: Auth::Inherit,
            settings: None,
            examples: Vec::new(),
            extra: BTreeMap::new(),
        })),
    }
}

/// One row's item: a problem row names a file, not an item.
fn row_item_id(row: &TreeRow) -> Option<ItemId> {
    match row {
        TreeRow::Collection { id, .. }
        | TreeRow::Folder { id, .. }
        | TreeRow::Request { id, .. } => Some(*id),
        TreeRow::Problem { .. } => None,
    }
}

/// Which container orders each item (REQ-WSP-01) — what "the item's parent" means
/// everywhere in this file. A child two parents both claim keeps the first (in id
/// order), which is what the tree draws it under too.
pub(crate) fn parent_map(items: &BTreeMap<ItemId, Item>) -> BTreeMap<ItemId, ItemId> {
    let mut parents = BTreeMap::new();
    for (id, item) in items {
        if let Some(order) = container_order(&item.body) {
            for child in order {
                parents.entry(*child).or_insert(*id);
            }
        }
    }
    parents
}

/// Append `id` to a container's `order`.
fn append_child(body: &mut ItemBody, id: ItemId) {
    if let Some(order) = container_order_mut(body) {
        order.push(id);
    }
}

/// Put `id` in a container's `order` right after `after`, where the user sees the copy
/// land beside its original; `after` gone means the end of the list.
fn insert_after(body: &mut ItemBody, after: ItemId, id: ItemId) {
    let Some(order) = container_order_mut(body) else {
        return;
    };
    match order.iter().position(|child| *child == after) {
        Some(position) => order.insert(position + 1, id),
        None => order.push(id),
    }
}

/// Take `id` out of a container's `order`.
pub(crate) fn remove_child(body: &mut ItemBody, id: ItemId) {
    if let Some(order) = container_order_mut(body) {
        order.retain(|child| *child != id);
    }
}

/// Set a container's `order` — the duplicate's copy of a source's children, and
/// [`crate::reorder`]'s moves.
pub(crate) fn set_order(body: &mut ItemBody, order: Vec<ItemId>) {
    if let Some(current) = container_order_mut(body) {
        *current = order;
    }
}

/// [`crate::tree::container_order`], for the side that writes the arrays.
fn container_order_mut(body: &mut ItemBody) -> Option<&mut Vec<ItemId>> {
    match body {
        ItemBody::Collection(collection) => Some(&mut collection.order),
        ItemBody::Folder(folder) => Some(&mut folder.order),
        _ => None,
    }
}

/// One duplicate run — of the source as the disk holds it now (S40a fix 2): the item,
/// its subtree and its place all come from the fresh read inside the write, never from
/// the snapshot the verb was aimed at. A failed copy removes everything it wrote
/// (S40a fix 1's rollback) and the toast names the path and the reason.
fn duplicate_work(root: &Path, source_id: ItemId, label: &str) -> Option<String> {
    let fresh = match workspace_store::open(root) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Some(format!("Could not duplicate “{label}”: {error}"));
        }
    };
    let Some(source) = fresh.items.get(&source_id).cloned() else {
        return Some(format!(
            "Could not duplicate “{label}”: it is no longer in the workspace"
        ));
    };
    let name = source.name();
    let source_path = source.path.clone().unwrap_or_default();
    let Some(source_id_in_file) = source.id() else {
        // A raw request kind this build does not model with no `id` in its JSON cannot
        // be given a new one (REQ-MODEL-02 keeps it verbatim).
        return Some(format!(
            "Could not duplicate “{name}” ({}): the file carries no id to copy from",
            source_path.display()
        ));
    };
    let copy_name = format!("Copy of {name}");
    let mut written = Vec::new();
    let copy = duplicate_subtree(
        root,
        &fresh.items,
        &source,
        source.dir.as_path(),
        Some(copy_name.clone()),
        &mut written,
    );
    let (copy_id, _) = match copy {
        Ok(copy) => copy,
        Err(error) => {
            // A duplicate that cannot finish leaves nothing behind: the files this copy
            // wrote go again (S40a fix 2), and the toast names the path and the reason.
            let mut cleanup_failure = None;
            for written in &written {
                if let Err(cleanup) = remove_written_file(root, written) {
                    cleanup_failure = Some(cleanup);
                }
            }
            let mut message = format!(
                "Could not duplicate “{name}” ({}): {error}",
                source_path.display()
            );
            if let Some(cleanup) = cleanup_failure {
                message.push_str(&format!(
                    " — and the partial copy could not be removed: {cleanup}"
                ));
            }
            return Some(message);
        }
    };
    // The parent as the disk holds it now — the same fresh read the copy came from.
    let Some(mut parent) = parent_map(&fresh.items)
        .get(&source_id)
        .copied()
        .and_then(|parent| fresh.items.get(&parent).cloned())
    else {
        // A root or an orphan: no `order` claims it (REQ-WSP-01).
        return None;
    };
    insert_after(&mut parent.body, source_id_in_file, copy_id);
    match workspace_store::save_item(root, &parent) {
        Ok(_) => None,
        Err(error) => Some(format!(
            "Could not add “{copy_name}” beside “{name}” ({}): {error}",
            source_path.display()
        )),
    }
}

/// Copy one item and its whole subtree, with a fresh id per file. The copy root takes
/// `name` (the "Copy of …"); the children keep theirs. Every copy is written into
/// `dir` — the copy root beside its original, and each child into its copied parent's
/// own `<slug>` directory (REQ-WSP-01). Every file written lands in `written`, so a
/// failure can take it all back. Returns the copy root's new id and where its file
/// landed.
fn duplicate_subtree(
    root: &Path,
    items: &BTreeMap<ItemId, Item>,
    source: &Item,
    dir: &Path,
    name: Option<String>,
    written: &mut Vec<PathBuf>,
) -> Result<(ItemId, PathBuf), workspace_store::SaveError> {
    let new_id = ItemId::new();
    let mut copy = source.clone();
    reidentify(&mut copy.body, new_id);
    if let Some(name) = name {
        copy.set_name(&name);
    }
    copy.dir = dir.to_path_buf();
    // The copy is a new file: `place` gives it a free slug beside its neighbours.
    copy.path = None;
    // The copy starts out childless (S40a fix 2): its `order` must never claim the
    // source's children — a child copy that fails would leave it claiming files it
    // does not own. The children are written next and the container is written once
    // more with their new ids after every one of them has landed.
    set_order(&mut copy.body, Vec::new());
    let saved = workspace_store::save_item(root, &copy)?;
    written.push(saved.clone());

    if let Some(children) = container_order(&source.body) {
        let Some(children_dir) = saved.parent().map(Path::to_path_buf) else {
            return Err(workspace_store::SaveError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a saved item's file always sits in a directory",
            )));
        };
        let mut copied_order = Vec::new();
        for child in children {
            if let Some(child) = items.get(child) {
                let (child_id, _) =
                    duplicate_subtree(root, items, child, &children_dir, None, written)?;
                copied_order.push(child_id);
            }
        }
        let mut placed = copy;
        set_order(&mut placed.body, copied_order);
        placed.path = Some(saved.clone());
        workspace_store::save_item(root, &placed)?;
    }
    Ok((new_id, saved))
}

/// Give an item's body a new identity — a duplicate is a new artifact (REQ-WSP-01), and
/// a raw request kind keeps its shape and swaps only the `id` key (REQ-MODEL-02).
fn reidentify(body: &mut ItemBody, id: ItemId) {
    match body {
        ItemBody::Workspace(workspace) => workspace.id = id,
        ItemBody::Collection(collection) => collection.id = id,
        ItemBody::Folder(folder) => folder.id = id,
        ItemBody::Environment(environment) => environment.id = id,
        ItemBody::Request(Request::Http(request)) => request.id = id,
        ItemBody::Request(Request::Unsupported(raw)) => {
            if let Some(object) = raw.as_object_mut() {
                object.insert(String::from("id"), serde_json::Value::from(id.to_string()));
            }
        }
    }
}

/// One delete run — of the item as the disk holds it now (S40a fix 2): its file (and,
/// for a container, its whole subtree) is resolved from the fresh read, so a rename
/// queued ahead of this verb cannot leave the file behind under its new place. The item
/// leaves its parent's `order` first — an `order` must never name a file that is gone.
fn delete_work(root: &Path, id: ItemId, label: &str) -> Option<String> {
    let fresh = match workspace_store::open(root) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Some(format!("Could not delete “{label}”: {error}"));
        }
    };
    let Some(item) = fresh.items.get(&id).cloned() else {
        return Some(format!(
            "Could not delete “{label}”: it is no longer in the workspace"
        ));
    };
    let name = item.name();
    let path = item.path.clone().unwrap_or_default();
    if let Some(parent) = parent_map(&fresh.items)
        .get(&id)
        .copied()
        .and_then(|parent| fresh.items.get(&parent).cloned())
    {
        let mut parent = parent;
        remove_child(&mut parent.body, id);
        if let Err(error) = workspace_store::save_item(root, &parent) {
            return Some(format!(
                "Could not delete “{name}” ({}): {error}",
                path.display()
            ));
        }
    }
    match remove_item_files(root, &item) {
        Ok(()) => None,
        Err(error) => Some(format!(
            "Could not delete “{name}” ({}): {error}",
            path.display()
        )),
    }
}

/// The item `id` as the disk holds it now (S40a fix 1): every write applies to this,
/// never to the snapshot its verb was aimed at. A workspace that cannot be read is an
/// error with the real reason (S40a fix 2), never a silent `None`.
fn current_item(root: &Path, id: ItemId, label: &str) -> Result<Option<Item>, String> {
    let snapshot = workspace_store::open(root)
        .map_err(|error| format!("Could not read the workspace for “{label}”: {error}"))?;
    Ok(snapshot.items.get(&id).cloned())
}

/// Remove one file a failed duplicate wrote, leaving nothing of the copy behind. The
/// path came from this run's own `save_item`, and the same confinement guard runs
/// before it goes.
fn remove_written_file(root: &Path, relative: &Path) -> Result<(), String> {
    confined_to_root(root, relative).map_err(|error| format!("{}: {error}", relative.display()))?;
    match std::fs::remove_file(root.join(relative)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{}: {error}", relative.display())),
        Ok(()) => Ok(()),
    }
}

/// Remove the item's file — or, for a folder or collection, its `<slug>` directory and
/// everything in it (REQ-WSP-01's one subtree per container; a rename moves that same
/// directory wholesale). An item never written has nothing to remove.
///
/// Deletion is this crate's own file work — `workspace_store` writes items but does not
/// remove them — so it carries its own confinement: the path must be plain names inside
/// the root and no directory on the way may be a symlink, the guard `save_item` keeps
/// private (an untrusted workspace can point a link anywhere).
fn remove_item_files(root: &Path, item: &Item) -> std::io::Result<()> {
    let Some(relative) = item.path.as_deref() else {
        return Ok(());
    };
    confined_to_root(root, relative)?;
    let container = matches!(item.body, ItemBody::Collection(_) | ItemBody::Folder(_));
    if container {
        let Some(slug_dir) = relative.parent() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} has no slug directory to remove", relative.display()),
            ));
        };
        if slug_dir.as_os_str().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} has no slug directory to remove", relative.display()),
            ));
        }
        confined_to_root(root, slug_dir)?;
        let full = root.join(slug_dir);
        match std::fs::symlink_metadata(&full) {
            // `symlink_metadata` reports a link as itself, so this only ever removes a
            // real directory — and `remove_dir_all` takes the subtree with it.
            Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(full),
            Ok(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "{} is not a directory this build will remove",
                    full.display()
                ),
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    } else {
        match std::fs::remove_file(root.join(relative)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

/// Refuse a path that would take the removal outside the workspace root: it climbs out
/// of the root, anchors outside it, or a directory on the way is a symlink.
fn confined_to_root(root: &Path, relative: &Path) -> std::io::Result<()> {
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "{} would remove outside the workspace root",
                relative.display()
            ),
        ));
    }
    let mut walk = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        walk.push(component.as_os_str());
        let last = index + 1 == components.len();
        match std::fs::symlink_metadata(&walk) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "{} is a link; this build removes nothing through one",
                        walk.display()
                    ),
                ));
            }
            Ok(metadata) if !last && !metadata.is_dir() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("{} is a file on the way to the removal", walk.display()),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// The confirm dialog behind `Delete` (PRD §3): one [`ui::Modal`] with the two things a
/// destructive confirm owes the user — Cancel that changes nothing, and a Confirm that
/// says what it destroys, scope and irreversibility both (the history panel's clear
/// modal's shape). Escape and a click outside dismiss through the modal layer without
/// deleting.
pub(crate) struct DeleteItemModal {
    tree: WeakEntity<CollectionsTree>,
    id: ItemId,
    name: SharedString,
    body: SharedString,
    // Kept on the view (not built in render) so the modal layer can take keyboard focus
    // on open and give it back on dismiss.
    focus_handle: FocusHandle,
}

impl DeleteItemModal {
    fn new(
        tree: WeakEntity<CollectionsTree>,
        id: ItemId,
        name: SharedString,
        is_container: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let body = if is_container {
            format!(
                "This removes “{name}” and everything inside it from the workspace. It cannot be undone."
            )
        } else {
            format!("This removes “{name}” from the workspace. It cannot be undone.")
        };
        Self {
            tree,
            id,
            name,
            body: SharedString::from(body),
            focus_handle: cx.focus_handle(),
        }
    }

    /// Cancel: dismiss without touching the workspace.
    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    /// Confirm: the one path that deletes, then dismisses.
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.id;
        if self
            .tree
            .update(cx, |tree, cx| tree.confirm_delete(id, window, cx))
            .is_err()
        {
            // Handled: the panel went away with its window; there is nothing to delete
            // into.
        }
        cx.emit(DismissEvent);
    }
}

impl Render for DeleteItemModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus_handle).child(
            Modal::new(format!("Delete “{}”?", self.name))
                .content(Label::new(self.body.clone()).size(LabelSize::Small))
                .footer(
                    h_flex()
                        .justify_end()
                        .gap(DynamicSpacing::Base04.rems(cx))
                        // Both buttons are labelled text buttons (their accessible
                        // names) and `ui::Button` keeps each one a tab stop Enter and
                        // Space activate, so the dialog is fully operable from the
                        // keyboard without this crate binding a key.
                        .child(
                            div()
                                .id("cancel-delete-item")
                                .debug_selector(|| DELETE_CANCEL_BUTTON_SELECTOR.to_owned())
                                .child(
                                    Button::new(
                                        "cancel-delete-item",
                                        SharedString::new_static("Cancel"),
                                    )
                                    .style(ButtonStyle::Subtle)
                                    .on_click(cx.listener(
                                        |this, _event, _window, cx| {
                                            this.cancel(cx);
                                        },
                                    )),
                                ),
                        )
                        .child(
                            div()
                                .id("confirm-delete-item")
                                .debug_selector(|| DELETE_CONFIRM_BUTTON_SELECTOR.to_owned())
                                .child(
                                    Button::new(
                                        "confirm-delete-item",
                                        SharedString::new_static("Delete"),
                                    )
                                    .style(ButtonStyle::Tinted(TintColor::Error))
                                    .on_click(cx.listener(
                                        |this, _event, window, cx| {
                                            this.confirm(window, cx);
                                        },
                                    )),
                                ),
                        ),
                ),
        )
    }
}

impl Focusable for DeleteItemModal {
    /// The modal is one tab stop at the top: its buttons carry the tab order after it.
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for DeleteItemModal {}

impl ModalView for DeleteItemModal {}
