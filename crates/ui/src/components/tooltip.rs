//! What a control does, shown when the pointer rests on it.
//!
//! A [`Tooltip`] is both the data a component stores and the element that draws: the same
//! value renders inline in the story and, through [`Tooltip::builder`], behind GPUI's
//! hover tooltip. `for_action` reads the keybinding at hover time, so the hint always
//! matches the keymap. Every icon-only control carries one (REQ-UI-07).

use gpui::{
    Action, AnyView, App, AppContext, Context, InteractiveElement, IntoElement, ParentElement,
    Render, RenderOnce, SharedString, Styled, Window, div,
};

use crate::color::Color;
use crate::components::key_binding::KeyBinding;
use crate::components::label::{Label, LabelSize};
use crate::elevation::StyledExt;
use crate::spacing::DynamicSpacing;
use crate::typography::StyledTypography;
use crate::units::px_to_rems;
use crate::v_flex;

/// The widest a tooltip grows before its text wraps: long values belong in the control
/// they describe, not in a balloon.
const MAX_WIDTH_PX: f32 = 320.;

/// Transparent space above a hover tooltip. GPUI anchors the tooltip one pixel from the
/// pointer, so without it the balloon is drawn over the control being hovered (#231);
/// this clears the pointer's arrow and the lower half of a 40 px row.
const POINTER_CLEARANCE_PX: f32 = 24.;

/// Test hook: `VisualTestContext::debug_bounds` finds a shown tooltip under this key.
pub const TOOLTIP_SELECTOR: &str = "tooltip";

/// The factory a component stores to build its hover tooltip.
pub type TooltipBuilder = Box<dyn Fn(&mut Window, &mut App) -> AnyView>;

/// The message a control shows on hover: a title, an optional second line, and the keys
/// that run it.
#[derive(Clone, Debug)]
pub struct Tooltip {
    title: SharedString,
    meta: Option<SharedString>,
    key_hint: Option<KeyBinding>,
    /// Suppressed while the control's own popover is open ([`Self::hidden`]).
    hidden: bool,
}

impl Tooltip {
    /// A one-line tooltip.
    pub fn text(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            meta: None,
            key_hint: None,
            hidden: false,
        }
    }

    /// A tooltip with a second line: what the value is, or what will happen.
    pub fn with_meta(title: impl Into<SharedString>, meta: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            meta: Some(meta.into()),
            key_hint: None,
            hidden: false,
        }
    }

    /// A tooltip that also teaches the keybinding for `action`.
    pub fn for_action(title: impl Into<SharedString>, action: &dyn Action) -> Self {
        Self {
            title: title.into(),
            meta: None,
            key_hint: Some(KeyBinding::for_action(action)),
            hidden: false,
        }
    }

    /// Keep the hover balloon away while `hidden` — how a control hides its tooltip
    /// while its own popover is open (#265): with the pointer resting on the anchor,
    /// the balloon would paint over the menu the pointer is really for.
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    /// Whether the hover balloon should stay away. The button components honor this by
    /// attaching no hover tooltip at all, which is also what clears one already on
    /// screen (GPUI drops the active tooltip when its builder goes away).
    pub(crate) fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// The title, which icon-only controls use as their accessible label.
    pub fn title(&self) -> &SharedString {
        &self.title
    }

    /// Fold the full value of a truncated control into this tooltip, so the value is
    /// always readable (`references/components.md`: every truncated text carries its full
    /// value). The value becomes the meta line — "what the value is" — unless this tooltip
    /// already *is* the value. The key hint is kept.
    pub(crate) fn with_full_value(mut self, value: SharedString) -> Self {
        if self.title != value {
            self.meta = Some(match self.meta {
                Some(meta) => format!("{value} — {meta}").into(),
                None => value,
            });
        }
        self
    }

    /// Draw it as a view, for GPUI's hover tooltip.
    pub fn into_any_view(self, cx: &mut App) -> AnyView {
        cx.new(|_| TooltipView(self)).into()
    }

    /// The [`TooltipBuilder`] a component stores.
    pub fn builder(self) -> TooltipBuilder {
        Box::new(move |_window, cx| self.clone().into_any_view(cx))
    }
}

/// The adapter GPUI's `AnyView` tooltip needs: [`Tooltip`] is a value, this is the view
/// that draws one.
struct TooltipView(Tooltip);

impl Render for TooltipView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .pt(px_to_rems(POINTER_CLEARANCE_PX))
            .child(self.0.clone().render(window, cx))
    }
}

impl RenderOnce for Tooltip {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut tooltip = v_flex()
            .debug_selector(|| TOOLTIP_SELECTOR.into())
            .elevation_2(cx)
            .font_ui(cx)
            .max_w(px_to_rems(MAX_WIDTH_PX))
            .px(DynamicSpacing::Base04.rems(cx))
            .py(DynamicSpacing::Base03.rems(cx))
            .gap(DynamicSpacing::Base01.rems(cx))
            .child(Label::new(self.title).size(LabelSize::Small));
        if let Some(meta) = self.meta {
            tooltip = tooltip.child(Label::new(meta).size(LabelSize::XSmall).color(Color::Muted));
        }
        if let Some(key_hint) = self.key_hint {
            tooltip = tooltip.child(key_hint);
        }
        tooltip
    }
}
