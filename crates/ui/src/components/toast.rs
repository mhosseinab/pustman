//! A message that appears, says its piece and gets out of the way.
//!
//! [`Toast`] is the message itself; [`Toasts`] is the stack that owns the timers. A toast
//! that must be read stays until dismissed; one that merely reports success goes away on
//! its own. Errors never use a timer — something failed, the user decides when to let it
//! go (`references/ux-checklist.md`: "errors are actionable ... nothing fails silently").

use std::time::Duration;

use gpui::{
    App, ClickEvent, Context, ElementId, EventEmitter, InteractiveElement, IntoElement,
    ParentElement, Render, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Task,
    Window, div, prelude::FluentBuilder,
};

use crate::color::{Color, Severity};
use crate::components::behavior::Clickable;
use crate::components::button::{Button, ButtonSize, ButtonStyle, ClickHandler, IconButton};
use crate::components::label::{Label, LabelSize};
use crate::components::tooltip::Tooltip;
use crate::elevation::StyledExt;
use crate::h_flex;
use crate::icons::{Icon, IconName, IconSize};
use crate::spacing::DynamicSpacing;
use crate::typography::StyledTypography;
use crate::v_flex;

/// One message in the stack.
#[derive(Clone, IntoElement)]
pub struct Toast {
    id: ElementId,
    message: SharedString,
    severity: Option<Severity>,
    action_label: Option<SharedString>,
    on_action: Option<ClickHandler>,
    on_dismiss: Option<ClickHandler>,
}

impl std::fmt::Debug for Toast {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Toast")
            .field("id", &self.id)
            .field("message", &self.message)
            .field("severity", &self.severity)
            .finish()
    }
}

impl Toast {
    /// A neutral message: "Copied", "Saved".
    pub fn new(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            message: message.into(),
            severity: None,
            action_label: None,
            on_action: None,
            on_dismiss: None,
        }
    }

    /// How serious it is: picks the icon and the tint.
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = Some(severity);
        self
    }

    /// Offer one thing the user can do about it ("Undo", "Open settings").
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.action_label = Some(label.into());
        self.on_action = Some(std::rc::Rc::new(handler));
        self
    }

    /// Wire the close button. [`Toasts::push`] does this itself.
    pub fn on_dismiss(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(std::rc::Rc::new(handler));
        self
    }

    fn icon_name(&self) -> IconName {
        match self.severity {
            Some(Severity::Error) => IconName::CircleAlert,
            Some(Severity::Warning) => IconName::TriangleAlert,
            Some(Severity::Success) => IconName::CircleCheck,
            Some(Severity::Info) | None => IconName::Info,
        }
    }
}

impl RenderOnce for Toast {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let icon_name = self.icon_name();
        let Toast {
            id,
            message,
            severity,
            action_label,
            on_action,
            on_dismiss,
        } = self;
        let full_message = message.clone();
        // Test hooks: `VisualTestContext::debug_bounds` finds the message and the action
        // button under the keys the two selector functions in this module hand out.
        let message_selector = toast_message_selector(&message);
        let action_selector = toast_action_selector(&message);

        let mut toast = h_flex()
            .id(id.clone())
            .font_ui(cx)
            .elevation_2(cx)
            .items_center()
            .gap(DynamicSpacing::Base03.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .py(DynamicSpacing::Base03.rems(cx))
            .min_w_0()
            .tooltip(Tooltip::text(full_message).builder())
            .child(Icon::new(icon_name).size(IconSize::Small))
            .child(
                div()
                    .min_w_0()
                    .debug_selector(move || message_selector)
                    .child(Label::new(message).size(LabelSize::Small).truncate()),
            )
            .map(|this| match severity {
                Some(severity) => this
                    .bg(severity.background(cx))
                    .border_color(severity.border(cx))
                    .text_color(severity.color(cx)),
                None => this.text_color(Color::Default.color(cx)),
            });

        if let Some(action_label) = action_label {
            toast = toast.child(
                div().debug_selector(move || action_selector).child(
                    Button::new((id.clone(), "action"), action_label)
                        .style(ButtonStyle::Subtle)
                        .size(ButtonSize::Compact)
                        .on_click(move |event, window, cx| {
                            if let Some(on_action) = &on_action {
                                on_action(event, window, cx);
                            }
                        }),
                ),
            );
        }

        if let Some(on_dismiss) = on_dismiss {
            toast = toast.child(
                IconButton::new((id, "dismiss"), IconName::X, Tooltip::text("Dismiss"))
                    .size(ButtonSize::Compact)
                    .icon_size(IconSize::XSmall)
                    .on_click(move |event, window, cx| on_dismiss(event, window, cx)),
            );
        }

        toast
    }
}

/// The debug key a [`Toast`]'s message renders under — the test hook that lets
/// `VisualTestContext::debug_bounds` prove one exact message is on screen (the `tab.rs`
/// pattern). It is anchored to the message so a caller and its test share one string
/// without knowing the handle [`Toasts::push`] re-keys the toast to.
pub fn toast_message_selector(message: &str) -> String {
    format!("toast-message-{message}")
}

/// The debug key a [`Toast`]'s action button renders under (see [`toast_message_selector`]).
pub fn toast_action_selector(message: &str) -> String {
    format!("toast-action-{message}")
}

/// What a [`Toasts`] stack tells its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastEvent {
    /// A toast left the stack — by the user's hand or by its own timer.
    Dismissed { handle: usize },
}

impl EventEmitter<ToastEvent> for Toasts {}

struct ToastEntry {
    handle: usize,
    toast: Toast,
    /// Held so the auto-dismiss timer keeps running; dropping it cancels the timer, which
    /// is what [`Toasts::dismiss`] does on purpose.
    _auto_dismiss: Option<Task<()>>,
}

/// The stack of toasts on screen, and the timers that clear them.
pub struct Toasts {
    entries: Vec<ToastEntry>,
    next_handle: usize,
}

impl Toasts {
    /// An empty stack.
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            entries: Vec::new(),
            next_handle: 0,
        }
    }

    /// Show `toast`. `Some(duration)` dismisses it on its own; `None` leaves it until the
    /// user dismisses it. Returns the handle to dismiss it by.
    pub fn push(
        &mut self,
        mut toast: Toast,
        duration: Option<Duration>,
        cx: &mut Context<Self>,
    ) -> usize {
        let handle = self.next_handle;
        self.next_handle += 1;

        // Re-key the toast from the stack's handle: callers pick ids like
        // "request-failed" and can push the same toast twice, but element ids (and the
        // action/dismiss ids derived from them) must stay unique per frame — two toasts
        // with one id would share element state and even a focus handle.
        toast.id = ElementId::from(("toast", handle));

        toast = toast.on_dismiss(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
            this.dismiss(handle, cx);
        }));

        let auto_dismiss = duration.map(|duration| {
            let timer = cx.background_executor().timer(duration);
            cx.spawn(async move |this, cx| {
                timer.await;
                this.update(cx, |this, cx| {
                    this.dismiss(handle, cx);
                })
                .ok();
            })
        });

        self.entries.push(ToastEntry {
            handle,
            toast,
            _auto_dismiss: auto_dismiss,
        });
        cx.notify();
        handle
    }

    /// Take a toast off the stack, cancelling its timer.
    pub fn dismiss(&mut self, handle: usize, cx: &mut Context<Self>) {
        self.entries.retain(|entry| entry.handle != handle);
        cx.emit(ToastEvent::Dismissed { handle });
        cx.notify();
    }

    /// How many toasts are on screen.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the stack is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Render for Toasts {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .font_ui(cx)
            .gap(DynamicSpacing::Base02.rems(cx))
            .children(self.entries.iter().map(|entry| entry.toast.clone()))
    }
}

#[cfg(test)]
#[path = "toast_tests.rs"]
mod tests;
