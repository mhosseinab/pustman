//! Toasts: the little messages that appear, say their piece and get out of the way
//! (PRD §3). [`Workspace::show_toast`] is the one door in — it builds an [`ui::Toast`]
//! and hands it to the [`ui::Toasts`] stack the workspace owns. The stack keeps the
//! auto-dismiss timers (as stored [`gpui::Task`]s); this module decides how long a toast
//! lives and where the stack sits on screen.

use std::rc::Rc;
use std::time::Duration;

use gpui::{AnyElement, App, Context, Entity, Window};
use ui::prelude::*;

use crate::Workspace;
use crate::status_bar::STATUS_BAR_HEIGHT_PX;

/// How long a toast that merely reports something stays on screen before it dismisses
/// itself. A [`Severity::Error`] ignores it: something failed, and the user decides
/// when to let that go (AGENTS.md › Errors must reach the user).
pub const TOAST_DURATION: Duration = Duration::from_secs(5);

/// The stable id [`Workspace::show_toast`] keys its toasts by. [`ui::Toasts::push`]
/// re-keys every toast to its own handle, so this only has to name the caller.
const TOAST_ID: &str = "workspace-toast";

/// The toast stack's element id and test selector: one singleton region of the shell.
const TOASTS_ID: &str = "workspace-toasts";

/// What a toast's action handler sees: the click (mouse or keyboard) that ran it — the
/// shape [`ui::ClickHandler`] takes, minus the event.
pub type ToastHandler = Rc<dyn Fn(&mut Window, &mut App)>;

/// One thing the user can do about a toast ("Undo", "Open settings"), run when they
/// click the toast's action button.
pub struct ToastAction {
    /// What the action button says.
    pub label: SharedString,
    /// What clicking it does.
    pub handler: ToastHandler,
}

impl Workspace {
    /// Show `message` as a toast of `level`, optionally offering one `action`.
    ///
    /// Info, success and warning toasts dismiss themselves after [`TOAST_DURATION`]; an
    /// error stays until the user dismisses it. Returns the handle to dismiss it by.
    pub fn show_toast(
        &mut self,
        message: impl Into<SharedString>,
        level: Severity,
        action: Option<ToastAction>,
        cx: &mut Context<Self>,
    ) -> usize {
        let message = message.into();
        let mut toast = Toast::new(TOAST_ID, message).severity(level);
        if let Some(action) = action {
            let handler = action.handler;
            toast = toast.action(action.label, move |_event, window, cx| handler(window, cx));
        }
        let duration = (level != Severity::Error).then_some(TOAST_DURATION);
        self.toasts
            .update(cx, |toasts, cx| toasts.push(toast, duration, cx))
    }

    /// Take a toast off the stack before its time is up.
    pub fn dismiss_toast(&mut self, handle: usize, cx: &mut Context<Self>) {
        self.toasts
            .update(cx, |toasts, cx| toasts.dismiss(handle, cx));
    }

    /// The toast stack the workspace owns — how tests and later steps read it.
    pub fn toasts(&self) -> &Entity<Toasts> {
        &self.toasts
    }

    /// The toast stack, floating in the bottom-right corner above the status bar.
    /// `workspace.rs` appends it as one of the root's two overlays, so a toast draws
    /// over the shell but under the modal layer.
    pub(crate) fn toasts_overlay(&self, cx: &App) -> AnyElement {
        let mut bottom_offset = px_to_rems(STATUS_BAR_HEIGHT_PX);
        bottom_offset += DynamicSpacing::Base02.rems(cx);
        div()
            .id(TOASTS_ID)
            .debug_selector(|| TOASTS_ID.to_owned())
            .absolute()
            .right(DynamicSpacing::Base04.rems(cx))
            .bottom(bottom_offset)
            .child(self.toasts.clone())
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "toast_layer_tests.rs"]
mod tests;
