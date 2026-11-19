//! The modal layer: one dialog at a time, over everything else (PRD §3).
//!
//! A dialog opts in with [`ModalView`]; [`ModalLayer`] hosts exactly one of them, holds
//! keyboard focus while it is open and hands focus back when it goes — on Escape, on a
//! click outside the dialog, or on the dialog's own [`gpui::DismissEvent`]. Zed's GPL
//! `workspace` crate has a modal layer of its own; this is our shape of the same idea.

use std::any::TypeId;

use gpui::{
    AnyElement, AnyView, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    KeyDownEvent, MouseDownEvent, Render, Subscription, Window, div,
};
use ui::prelude::*;

use crate::Workspace;

/// The overlay's element id and test selector: one singleton region of the shell.
const MODAL_LAYER_ID: &str = "modal-layer";

/// The element id of the box the open modal view sits in — what a click "outside the
/// modal" is measured against, and what tests find the dialog by.
const MODAL_CONTAINER_ID: &str = "modal-container";

/// A view the [`ModalLayer`] can host: renders, takes keyboard focus and says when it is
/// done by emitting [`DismissEvent`]. Implementers opt in with `impl ModalView for … {}`.
pub trait ModalView: Render + Focusable + EventEmitter<DismissEvent> {}

/// The modal on screen, with everything needed to take it down again.
pub struct ActiveModal {
    /// The modal view, type-erased so the layer can host any of them.
    view: AnyView,
    /// Which modal type this is — [`ModalLayer::toggle_modal`] closes a modal when asked
    /// to open a second one of the same type.
    type_id: TypeId,
    /// Where keyboard focus goes while the modal is open.
    focus_handle: FocusHandle,
    /// Where keyboard focus goes back to when the modal closes.
    previous_focus: Option<FocusHandle>,
    /// Held so the [`DismissEvent`] subscription keeps firing; dropping it when the modal
    /// goes is what detaches the dialog.
    _subscription: Subscription,
}

/// The modal layer: at most one modal at a time.
#[derive(Default)]
pub struct ModalLayer {
    active: Option<ActiveModal>,
}

impl ModalLayer {
    /// Open the modal `build` makes — or close it if a modal of that type is already
    /// open. Any other open modal goes first: one at a time.
    pub fn toggle_modal<V: ModalView>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Workspace>,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.type_id == TypeId::of::<V>())
        {
            self.dismiss(window, cx);
            return;
        }

        // One at a time: whatever was open gives focus back before the new modal takes it.
        self.dismiss(window, cx);

        let previous_focus = window.focused(cx);
        let view = cx.new(|cx| build(window, cx));
        let subscription = cx.subscribe_in::<V, DismissEvent>(
            &view,
            window,
            |workspace, _view, _event, window, cx| {
                workspace.modal_layer.dismiss(window, cx);
            },
        );
        let active = ActiveModal {
            type_id: TypeId::of::<V>(),
            focus_handle: view.read(cx).focus_handle(cx),
            view: view.into(),
            previous_focus,
            _subscription: subscription,
        };
        window.focus(&active.focus_handle, cx);
        self.active = Some(active);
        cx.notify();
    }

    /// Take the open modal down, if any, and give keyboard focus back where it came from.
    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Workspace>) {
        let Some(active) = self.active.take() else {
            return;
        };
        if let Some(previous_focus) = active.previous_focus {
            window.focus(&previous_focus, cx);
        }
        // `active` drops at the end of this method: that detaches the DismissEvent
        // subscription and lets the modal view go.
        cx.notify();
    }

    /// Whether a modal is on screen.
    pub fn is_open(&self) -> bool {
        self.active.is_some()
    }

    /// The open modal, if it is a `V`.
    pub fn active_modal<V: ModalView>(&self) -> Option<Entity<V>> {
        let active = self.active.as_ref()?;
        if active.type_id != TypeId::of::<V>() {
            return None;
        }
        active.view.clone().downcast::<V>().ok()
    }
}

impl Workspace {
    /// Show the modal `build` makes, or close it if one of that type is open
    /// ([`ModalLayer::toggle_modal`]).
    pub fn toggle_modal<V: ModalView>(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        build: impl FnOnce(&mut Window, &mut Context<V>) -> V,
    ) {
        self.modal_layer.toggle_modal(window, cx, build);
    }

    /// Close the open modal, if any ([`ModalLayer::dismiss`]).
    pub fn dismiss_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.modal_layer.dismiss(window, cx);
    }

    /// The modal layer — how tests and later steps read its state.
    pub fn modal_layer(&self) -> &ModalLayer {
        &self.modal_layer
    }

    /// The modal layer's overlay: while a modal is open, a full-size layer above
    /// everything that occludes what is behind it and hosts the modal centered. Escape
    /// or a click outside the modal dismisses it. `workspace.rs` appends it last, so it
    /// draws over the toasts too.
    pub(crate) fn modal_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(active) = self.modal_layer.active.as_ref() else {
            return div().into_any_element();
        };
        let modal_view = active.view.clone();

        div()
            .id(MODAL_LAYER_ID)
            .debug_selector(|| MODAL_LAYER_ID.to_owned())
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            // Escape closes the modal. There is no dismiss action to answer it with yet
            // (pustman_actions has none and this step may not add one), so the layer
            // takes the key itself until a later step moves it to an action.
            .on_key_down(cx.listener(|workspace, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    workspace.dismiss_modal(window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .id(MODAL_CONTAINER_ID)
                    .debug_selector(|| MODAL_CONTAINER_ID.to_owned())
                    .max_w_full()
                    .max_h_full()
                    .on_mouse_down_out(cx.listener(
                        |workspace, _event: &MouseDownEvent, window, cx| {
                            workspace.dismiss_modal(window, cx);
                        },
                    ))
                    .child(modal_view),
            )
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "modal_layer_tests.rs"]
mod tests;
