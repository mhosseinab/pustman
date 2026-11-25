//! The New Collection form (S48u1): what `collections::NewCollection` opens — from the
//! tree's keys, its row menu and anywhere else the action is dispatched (one
//! implementation). Name (required) and Description (optional); Create goes through the
//! tree's one write queue ([`CollectionsTree::create_collection`], S40a) and selects the
//! new collection's row once it lands.
//!
//! With no workspace open there is nothing to create a collection in, so the action
//! opens [`NoWorkspaceModal`] instead: it says so and offers New Workspace… and Open
//! Workspace…, rather than doing nothing.
//!
//! `Decision:` a name a sibling collection already uses is not an error: the form says
//! "will be saved as <name> (2)" (the first free number) and saves it under that name,
//! so the tree never shows two collections it cannot tell apart.
//!
//! `Decision:` Tab and Shift-Tab cycle through the dialog's own controls and wrap (the
//! New Workspace form's rule), so keyboard focus cannot fall onto the tree behind it.

use gpui::{
    DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Render, Subscription,
    WeakEntity,
};
use pustman_actions::workspace::{CreateWorkspace, OpenWorkspace};
use text_input::{TextArea, TextInput, TextInputEvent};
use ui::components::form_field::cycle_focus;
use ui::prelude::*;
use workspace::{ModalView, Workspace};

use crate::crud::NewCollection;
use crate::tree::CollectionsTree;

#[cfg(test)]
#[path = "new_collection_form_tests.rs"]
mod tests;

/// The stable field names [`FormField`] derives its test selectors from.
pub const NAME_FIELD: &str = "new-collection-name";
pub const DESCRIPTION_FIELD: &str = "new-collection-description";

/// Test hooks for the dialogs' buttons.
pub const CREATE_BUTTON_SELECTOR: &str = "new-collection-create";
pub const CANCEL_BUTTON_SELECTOR: &str = "new-collection-cancel";
pub const NEW_WORKSPACE_BUTTON_SELECTOR: &str = "no-workspace-new";
pub const OPEN_WORKSPACE_BUTTON_SELECTOR: &str = "no-workspace-open";

/// What the forms say.
pub const NAME_REQUIRED: &str = "Enter a name.";
pub const NO_WORKSPACE_MESSAGE: &str =
    "No workspace is open. Collections live in a workspace: create one or open one first.";

/// The description box's height: a few lines of text, like the design pack's text area.
const DESCRIPTION_HEIGHT_PX: f32 = 72.;
/// The dialogs' width (the New Workspace form's).
const FORM_WIDTH_PX: f32 = 480.;

/// Answer `NewCollection` wherever the keyboard is, not only in the focused tree: the
/// tree's own listener runs first when it has focus, and this app-level one otherwise.
pub(crate) fn init(cx: &mut App) {
    cx.on_action(new_collection_action);
}

/// `NewCollection` outside the tree: the active window's form, or the no-workspace
/// dialog. Deferred like the import action: the dispatch runs inside the window's update.
fn new_collection_action(_: &NewCollection, cx: &mut App) {
    let Some(window_handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        let opened = window_handle.update(cx, |root, window, cx| {
            let Ok(workspace) = root.downcast::<Workspace>() else {
                return;
            };
            match collections_tree(&workspace, cx) {
                Some(tree) => {
                    tree.update(cx, |tree, cx| tree.open_new_collection_form(window, cx));
                }
                None => open_no_workspace(&workspace, window, cx),
            }
        });
        if opened.is_err() {
            // Handled: the window closed before the action ran; nobody is left to ask.
        }
    });
}

/// The Collections panel in `workspace`'s left dock.
fn collections_tree(workspace: &Entity<Workspace>, cx: &App) -> Option<Entity<CollectionsTree>> {
    workspace
        .read(cx)
        .left_dock()
        .read(cx)
        .panels()
        .iter()
        .find_map(|panel| panel.to_any_view().downcast::<CollectionsTree>().ok())
}

/// Show the no-workspace dialog over `workspace`.
fn open_no_workspace(workspace: &Entity<Workspace>, window: &mut Window, cx: &mut App) {
    workspace.update(cx, |workspace, cx| {
        if workspace
            .modal_layer()
            .active_modal::<NoWorkspaceModal>()
            .is_none()
        {
            workspace.toggle_modal(window, cx, NoWorkspaceModal::new);
        }
    });
}

impl CollectionsTree {
    /// Open the New Collection form — or, with no workspace open, the dialog that offers
    /// one. A read-only workspace says why it takes no new collection.
    pub(crate) fn open_new_collection_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Some(workspace)) = window.root::<Workspace>() else {
            return;
        };
        if self.snapshot.root.as_os_str().is_empty() {
            open_no_workspace(&workspace, window, cx);
            return;
        }
        if self.writable_root(window, cx).is_none() {
            // Reported: the read-only workspace already said why.
            return;
        }
        let tree = cx.weak_entity();
        let existing = self.collection_names();
        workspace.update(cx, |workspace, cx| {
            if workspace
                .modal_layer()
                .active_modal::<NewCollectionForm>()
                .is_some()
            {
                return;
            }
            workspace.toggle_modal(window, cx, |window, cx| {
                NewCollectionForm::new(tree, existing, window, cx)
            });
        });
    }
}

/// The New Collection dialog.
pub struct NewCollectionForm {
    tree: WeakEntity<CollectionsTree>,
    /// The collections already at the workspace root, for the "saved as" note.
    existing: Vec<String>,
    name: Entity<TextInput>,
    description: Entity<TextArea>,
    create_focus: FocusHandle,
    cancel_focus: FocusHandle,
    /// The user has been in Name (or tried Create), so an empty name is worth saying.
    name_touched: bool,
    _subscriptions: Vec<Subscription>,
}

impl NewCollectionForm {
    fn new(
        tree: WeakEntity<CollectionsTree>,
        existing: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let name = cx.new(|cx| {
            TextInput::new(window, cx).with_placeholder(SharedString::new_static("e.g. Orders"))
        });
        let description = cx.new(|cx| TextArea::new(window, cx));
        let subscriptions = vec![cx.subscribe_in(
            &name,
            window,
            |form, _input, event: &TextInputEvent, window, cx| match event {
                TextInputEvent::Changed => {
                    form.name_touched = true;
                    cx.notify();
                }
                TextInputEvent::Submitted => form.submit(window, cx),
            },
        )];
        Self {
            tree,
            existing,
            name,
            description,
            create_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            name_touched: false,
            _subscriptions: subscriptions,
        }
    }

    /// The Name and Description fields — how the tests fill the form.
    #[cfg(test)]
    pub(crate) fn fields(&self) -> (Entity<TextInput>, Entity<TextArea>) {
        (self.name.clone(), self.description.clone())
    }

    fn name_text(&self, cx: &App) -> String {
        self.name.read(cx).text().trim().to_owned()
    }

    /// The name the collection is saved under: the typed name, numbered when a sibling
    /// already has it (the module docs).
    pub fn saved_name(&self, cx: &App) -> String {
        unique_name(&self.name_text(cx), &self.existing)
    }

    pub fn can_create(&self, cx: &App) -> bool {
        !self.name_text(cx).is_empty()
    }

    /// Create, if valid (Enter in Name and the Create button).
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.name_touched = true;
        if !self.can_create(cx) {
            cx.notify();
            return;
        }
        let name = self.saved_name(cx);
        let description = self.description.read(cx).text();
        let description = (!description.trim().is_empty()).then_some(description);
        if self
            .tree
            .update(cx, |tree, cx| {
                tree.create_collection(name, description, window, cx);
            })
            .is_err()
        {
            // Handled: the panel went away with its window; there is nothing to create
            // into and the window's toasts went with it.
        }
        cx.emit(DismissEvent);
    }

    fn tab_order(&self, cx: &App) -> Vec<FocusHandle> {
        let mut order = vec![
            self.name.read(cx).focus_handle(cx),
            self.description.read(cx).focus_handle(cx),
        ];
        if self.can_create(cx) {
            order.push(self.create_focus.clone());
        }
        order.push(self.cancel_focus.clone());
        order
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let order = self.tab_order(cx);
        if cycle_focus(&order, event, window, cx) {
            cx.stop_propagation();
        }
    }
}

impl Render for NewCollectionForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let can_create = self.can_create(cx);
        let name = self.name_text(cx);
        let saved_name = self.saved_name(cx);
        let name_field = FormField::new(NAME_FIELD, "Name")
            .content(self.name.clone())
            .map(|field| {
                if self.name_touched && name.is_empty() {
                    field.error(NAME_REQUIRED)
                } else if saved_name != name {
                    field.hint(format!(
                        "“{name}” already exists. This one will be saved as “{saved_name}”."
                    ))
                } else {
                    field
                }
            });
        let description_field = FormField::new(DESCRIPTION_FIELD, "Description (optional)")
            .content(
                div()
                    .w_full()
                    .h(px_to_rems(DESCRIPTION_HEIGHT_PX))
                    .child(self.description.clone()),
            );
        let footer = h_flex()
            .w_full()
            .items_center()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(
                div().flex_1().min_w_0().child(
                    Label::new(if can_create {
                        "Enter to create"
                    } else {
                        "Enter a name to create"
                    })
                    .size(LabelSize::Small)
                    .color(Color::Muted),
                ),
            )
            .child(
                div()
                    .debug_selector(|| CANCEL_BUTTON_SELECTOR.to_owned())
                    .child(
                        Button::new("new-collection-cancel", "Cancel")
                            .style(ButtonStyle::Outlined)
                            .focus_handle(self.cancel_focus.clone())
                            .on_click(cx.listener(|_form, _event, _window, cx| {
                                cx.emit(DismissEvent);
                            })),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| CREATE_BUTTON_SELECTOR.to_owned())
                    .child(
                        Button::new("new-collection-create", "Create collection")
                            .style(ButtonStyle::Filled)
                            .disabled(!can_create)
                            .focus_handle(self.create_focus.clone())
                            .on_click(cx.listener(|form, _event, window, cx| {
                                form.submit(window, cx);
                            })),
                    ),
            );
        div()
            .id("new-collection-form")
            .w(px_to_rems(FORM_WIDTH_PX))
            .max_w_full()
            .on_key_down(cx.listener(Self::on_key_down))
            .child(
                Modal::new("New collection")
                    .content(
                        v_flex()
                            .w_full()
                            .gap(DynamicSpacing::Base08.rems(cx))
                            .child(name_field)
                            .child(description_field),
                    )
                    .footer(footer),
            )
    }
}

impl Focusable for NewCollectionForm {
    /// Name has focus when the form opens.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.name.read(cx).focus_handle(cx)
    }
}

impl EventEmitter<DismissEvent> for NewCollectionForm {}

impl ModalView for NewCollectionForm {}

/// What `NewCollection` shows with no workspace open: the reason, and the two ways to get
/// a workspace.
pub struct NoWorkspaceModal {
    new_focus: FocusHandle,
    open_focus: FocusHandle,
    cancel_focus: FocusHandle,
}

impl NoWorkspaceModal {
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            new_focus: cx.focus_handle(),
            open_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
        }
    }

    /// Close, then run `action` in this window: the workspace actions open their own
    /// form or dialog.
    fn hand_over(
        &mut self,
        action: Box<dyn gpui::Action>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DismissEvent);
        window.dispatch_action(action, cx);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let order = [
            self.new_focus.clone(),
            self.open_focus.clone(),
            self.cancel_focus.clone(),
        ];
        if cycle_focus(&order, event, window, cx) {
            cx.stop_propagation();
        }
    }
}

impl Render for NoWorkspaceModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let footer = h_flex()
            .w_full()
            .justify_end()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(
                Button::new("no-workspace-cancel", "Cancel")
                    .style(ButtonStyle::Outlined)
                    .focus_handle(self.cancel_focus.clone())
                    .on_click(cx.listener(|_modal, _event, _window, cx| cx.emit(DismissEvent))),
            )
            .child(
                div()
                    .debug_selector(|| OPEN_WORKSPACE_BUTTON_SELECTOR.to_owned())
                    .child(
                        Button::new("no-workspace-open", "Open workspace…")
                            .style(ButtonStyle::Outlined)
                            .focus_handle(self.open_focus.clone())
                            .on_click(cx.listener(|modal, _event, window, cx| {
                                modal.hand_over(Box::new(OpenWorkspace), window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| NEW_WORKSPACE_BUTTON_SELECTOR.to_owned())
                    .child(
                        Button::new("no-workspace-new", "New workspace…")
                            .style(ButtonStyle::Filled)
                            .focus_handle(self.new_focus.clone())
                            .on_click(cx.listener(|modal, _event, window, cx| {
                                modal.hand_over(Box::new(CreateWorkspace), window, cx);
                            })),
                    ),
            );
        div()
            .id("no-workspace-modal")
            .w(px_to_rems(FORM_WIDTH_PX))
            .max_w_full()
            .on_key_down(cx.listener(Self::on_key_down))
            .child(
                Modal::new("New collection")
                    .content(
                        Banner::new("no-workspace-message", NO_WORKSPACE_MESSAGE)
                            .severity(Severity::Info),
                    )
                    .footer(footer),
            )
    }
}

impl Focusable for NoWorkspaceModal {
    /// The primary way forward, New workspace…, has focus when the dialog opens.
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.new_focus.clone()
    }
}

impl EventEmitter<DismissEvent> for NoWorkspaceModal {}

impl ModalView for NoWorkspaceModal {}

/// `name`, or `name (n)` with the first `n` from 2 that no name in `existing` has.
/// Compared case-insensitively: "orders" and "Orders" read as the same collection.
fn unique_name(name: &str, existing: &[String]) -> String {
    let taken = |candidate: &str| {
        existing
            .iter()
            .any(|other| other.trim().eq_ignore_ascii_case(candidate))
    };
    if !taken(name) {
        return name.to_owned();
    }
    (2u32..)
        .map(|number| format!("{name} ({number})"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| name.to_owned())
}
