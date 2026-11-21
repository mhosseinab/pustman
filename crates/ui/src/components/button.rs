//! Push buttons and icon buttons.
//!
//! A button is a value ([`Button`]) that owns no state: the view around it owns the
//! [`gpui::FocusHandle`] and passes it in, which is what makes the button reachable with
//! Tab and activatable with Enter or Space. Both go through one handler, so the mouse and
//! the keyboard can never disagree about what a click does.

use std::rc::Rc;

use gpui::{
    AbsoluteLength, AnimationExt, App, ClickEvent, CursorStyle, ElementId, FocusHandle, Hsla,
    InteractiveElement, IntoElement, ParentElement, Rems, RenderOnce, Role, SharedString,
    StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
};
use theme::ActiveTheme;

use crate::color::{Color, Severity};
use crate::components::behavior::{Clickable, Disableable, FixedWidth, VisibleOnHover};
use crate::components::key_binding::KeyBinding;
use crate::components::label::{Label, LabelSize};
use crate::components::toggle::ACTIVATION_CONTEXT;
use crate::components::tooltip::Tooltip;
use crate::h_flex;
use crate::icons::{Icon, IconName, IconSize};
use crate::spacing::DynamicSpacing;
use crate::typography::StyledTypography;
use crate::units::px_to_rems;

/// What the click handler sees: a mouse click, or a keyboard activation.
pub type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The tint of a [`ButtonStyle::Tinted`] button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TintColor {
    /// The accent: "this is the next thing to do" without being the primary fill.
    Accent,
    /// This fails or destroys something.
    Error,
    /// Look at this first.
    Warning,
    /// This went well.
    Success,
}

impl TintColor {
    fn color(self, cx: &App) -> Hsla {
        match self {
            Self::Accent => cx.theme().colors().text_accent,
            Self::Error => Severity::Error.color(cx),
            Self::Warning => Severity::Warning.color(cx),
            Self::Success => Severity::Success.color(cx),
        }
    }

    fn background(self, cx: &App) -> Hsla {
        match self {
            Self::Accent => cx.theme().colors().element_selected,
            Self::Error => Severity::Error.background(cx),
            Self::Warning => Severity::Warning.background(cx),
            Self::Success => Severity::Success.background(cx),
        }
    }

    fn border(self, cx: &App) -> Hsla {
        match self {
            // The selected fill and accent text already mark an active toggle; an accent
            // outline on top read as a focus ring on every active chip.
            Self::Accent => cx.theme().colors().border_transparent,
            Self::Error => Severity::Error.border(cx),
            Self::Warning => Severity::Warning.border(cx),
            Self::Success => Severity::Success.border(cx),
        }
    }
}

/// How loud a button is. One [`ButtonStyle::Filled`] per region — that is the primary
/// action (Send).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonStyle {
    /// The primary action of its region: the accent fill.
    Filled,
    /// A button that carries a severity, e.g. `Tinted(TintColor::Error)` for Delete.
    Tinted(TintColor),
    /// A control outline over a transparent interior.
    Outlined,
    /// An outline that only appears on hover and focus.
    OutlinedGhost,
    /// The default: no frame until hovered.
    #[default]
    Subtle,
    /// Bare text in a toolbar.
    Transparent,
}

impl ButtonStyle {
    fn text_color(self, cx: &App) -> Hsla {
        match self {
            Self::Filled => cx.theme().colors().accent_foreground,
            Self::Tinted(tint) => tint.color(cx),
            Self::Outlined | Self::OutlinedGhost | Self::Subtle | Self::Transparent => {
                cx.theme().colors().text
            }
        }
    }

    fn background(self, cx: &App) -> Option<Hsla> {
        match self {
            Self::Filled => Some(cx.theme().colors().accent_background),
            Self::Tinted(tint) => Some(tint.background(cx)),
            Self::Outlined | Self::OutlinedGhost | Self::Subtle | Self::Transparent => None,
        }
    }

    fn border_color(self, cx: &App) -> Option<Hsla> {
        match self {
            Self::Filled => Some(cx.theme().colors().accent_background),
            Self::Tinted(tint) => Some(tint.border(cx)),
            Self::Outlined => Some(cx.theme().colors().border),
            Self::OutlinedGhost | Self::Subtle | Self::Transparent => None,
        }
    }

    /// Filled buttons fade on hover; framed and ghost buttons pick up a fill.
    fn fades_on_hover(self) -> bool {
        matches!(self, Self::Filled | Self::Tinted(_))
    }
}

/// How big a button is: its height at the default scale (`references/components.md`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonSize {
    /// 32 px: toolbar buttons and the URL bar's Send.
    Large,
    /// 28 px: toolbar rows.
    Medium,
    /// 22 px: dense table rows (a documented exception to the 24 px pointer target).
    #[default]
    Default,
    /// 18 px: inside other controls.
    Compact,
    /// No height of its own: the button hugs its content.
    None,
}

impl ButtonSize {
    fn height(self) -> Option<Rems> {
        match self {
            Self::Large => Some(px_to_rems(32.)),
            Self::Medium => Some(px_to_rems(28.)),
            Self::Default => Some(px_to_rems(22.)),
            Self::Compact => Some(px_to_rems(18.)),
            Self::None => None,
        }
    }

    fn padding_x(self) -> DynamicSpacing {
        match self {
            Self::Large | Self::Medium => DynamicSpacing::Base08,
            Self::Default | Self::Compact | Self::None => DynamicSpacing::Base04,
        }
    }

    fn label_size(self) -> LabelSize {
        match self {
            Self::Large | Self::Medium => LabelSize::Default,
            Self::Default | Self::None => LabelSize::Small,
            Self::Compact => LabelSize::XSmall,
        }
    }
}

/// A button with a text label.
#[derive(Clone, IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    style: ButtonStyle,
    size: ButtonSize,
    start_icon: Option<Icon>,
    end_icon: Option<Icon>,
    loading: bool,
    disabled: bool,
    width: Option<AbsoluteLength>,
    full_width: bool,
    key_binding: Option<KeyBinding>,
    tooltip: Option<Tooltip>,
    focus_handle: Option<FocusHandle>,
    tab_index: Option<isize>,
    on_click: Option<ClickHandler>,
    cursor_style: CursorStyle,
}

impl std::fmt::Debug for Button {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Button")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("style", &self.style)
            .field("size", &self.size)
            .field("loading", &self.loading)
            .field("disabled", &self.disabled)
            .finish()
    }
}

impl Button {
    /// A [`ButtonStyle::Subtle`] button of [`ButtonSize::Default`].
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            style: ButtonStyle::Subtle,
            size: ButtonSize::Default,
            start_icon: None,
            end_icon: None,
            loading: false,
            disabled: false,
            width: None,
            full_width: false,
            key_binding: None,
            tooltip: None,
            focus_handle: None,
            tab_index: None,
            on_click: None,
            cursor_style: CursorStyle::PointingHand,
        }
    }

    /// How loud the button is.
    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    /// How big it is.
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    /// An icon before the label.
    pub fn start_icon(mut self, icon: Icon) -> Self {
        self.start_icon = Some(icon);
        self
    }

    /// An icon after the label (a chevron on a dropdown trigger).
    pub fn end_icon(mut self, icon: Icon) -> Self {
        self.end_icon = Some(icon);
        self
    }

    /// Show the button as working: a spinner replaces the start icon and it stops taking
    /// clicks until it is turned off.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Show the keybinding that runs this button, after the label.
    pub fn key_binding(mut self, key_binding: KeyBinding) -> Self {
        self.key_binding = Some(key_binding);
        self
    }

    /// What to show when the pointer rests on it.
    pub fn tooltip(mut self, tooltip: Tooltip) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    /// The focus handle this button answers the keyboard with, for callers that own a
    /// tab order. Without one the button keeps its own handle per [`ElementId`] (GPUI
    /// stores it in the element state) and is still a tab stop that Enter and Space
    /// activate.
    ///
    /// With a handle, the handle itself carries the tab order: the button marks it a tab
    /// stop while it is enabled and applies [`Self::tab_index`] to it. GPUI ignores the
    /// element-level `tab_stop`/`tab_index` when a handle is supplied (it only reads them
    /// for handles it creates), so a caller-supplied handle that is not a tab stop leaves
    /// the button out of the tab order.
    pub fn focus_handle(mut self, focus_handle: FocusHandle) -> Self {
        self.focus_handle = Some(focus_handle);
        self
    }

    /// Where this button sits in its container's tab order.
    pub fn tab_index(mut self, tab_index: isize) -> Self {
        self.tab_index = Some(tab_index);
        self
    }
}

impl Clickable for Button {
    fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    fn cursor_style(mut self, style: CursorStyle) -> Self {
        self.cursor_style = style;
        self
    }
}

impl Disableable for Button {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl FixedWidth for Button {
    fn width(mut self, width: impl Into<AbsoluteLength>) -> Self {
        self.width = Some(width.into());
        self
    }

    fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let label = self.label.clone();
        let on_click = self.on_click.clone();
        let focus_handle = self.focus_handle.clone();
        // The label is truncated, so it must always be readable in the tooltip: either
        // the caller's tooltip folded around it or a tooltip of its own.
        let tooltip = match self.tooltip.clone() {
            Some(tooltip) => tooltip.with_full_value(label.clone()),
            None => Tooltip::text(label.clone()),
        };
        let style = self.style;
        let loading = self.loading;
        let disabled = self.disabled || loading;

        let mut content = h_flex()
            .gap(DynamicSpacing::Base03.rems(cx))
            .when(loading, |this| this.child(spinner(IconSize::Small)))
            .when_some(self.start_icon.filter(|_| !loading), |this, icon| {
                this.child(icon.color(if disabled {
                    Color::Disabled
                } else {
                    Color::Default
                }))
            })
            .child(
                Label::new(label.clone())
                    .size(self.size.label_size())
                    .truncate(),
            )
            .when_some(self.end_icon, |this, icon| this.child(icon));

        if let Some(key_binding) = self.key_binding {
            content = content.child(key_binding.color(if disabled {
                Color::Disabled
            } else {
                Color::Muted
            }));
        }

        h_flex()
            .id(self.id)
            .font_ui(cx)
            .key_context(ACTIVATION_CONTEXT)
            .role(Role::Button)
            .aria_label(label)
            .items_center()
            .flex_shrink_0()
            .px(self.size.padding_x().rems(cx))
            .when_some(self.size.height(), |this, height| this.h(height))
            .when_some(self.width, |this, width| this.w(width))
            .map(|this| {
                if self.full_width {
                    this.w_full().justify_center()
                } else {
                    this
                }
            })
            .border_1()
            .border_color(
                style
                    .border_color(cx)
                    .unwrap_or_else(|| cx.theme().colors().border_transparent),
            )
            .rounded_sm()
            .text_color(if disabled {
                Color::Disabled.color(cx)
            } else {
                style.text_color(cx)
            })
            .when_some(style.background(cx), |this, background| this.bg(background))
            .map(|this| match focus_handle.as_ref() {
                // With a caller's handle the handle's own tab_stop and tab_index decide
                // the tab order; the element-level ones only apply to handles GPUI creates.
                Some(handle) => {
                    let handle = handle.clone().tab_stop(!disabled);
                    let handle = match self.tab_index {
                        Some(index) => handle.tab_index(index),
                        None => handle,
                    };
                    this.track_focus(&handle)
                }
                None if on_click.is_some() => this.focusable().tab_stop(!disabled),
                None => this,
            })
            .focus_visible(|style_refinement| {
                style_refinement.border_color(cx.theme().colors().border_focused)
            })
            .when_some(self.tab_index, |this, index| this.tab_index(index))
            // A hidden tooltip — the control's own popover is open (#265) — attaches no
            // hover balloon at all, which is also what clears one already on screen.
            .when(!tooltip.is_hidden(), |this| this.tooltip(tooltip.builder()))
            .child(content)
            .map(|this| {
                if disabled {
                    this.cursor_not_allowed().opacity(0.6)
                } else if style.fades_on_hover() {
                    this.cursor(self.cursor_style)
                        .hover(|s| s.opacity(0.88))
                        .active(|s| s.opacity(0.76))
                        .on_click(move |event, window, cx| {
                            if let Some(on_click) = &on_click {
                                cx.stop_propagation();
                                on_click(event, window, cx);
                            }
                        })
                } else {
                    this.cursor(self.cursor_style)
                        .hover(|s| s.bg(cx.theme().colors().ghost_element_hover))
                        .active(|s| s.bg(cx.theme().colors().ghost_element_active))
                        .on_click(move |event, window, cx| {
                            if let Some(on_click) = &on_click {
                                cx.stop_propagation();
                                on_click(event, window, cx);
                            }
                        })
                }
            })
    }
}

/// The shape an [`IconButton`] is drawn in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconButtonShape {
    /// A square box with a small corner radius.
    #[default]
    Square,
    /// A round box.
    Rounded,
}

/// An icon-only button. Its [`Tooltip`] is mandatory — it is the button's discoverable
/// name *and* its accessible label, so an icon button can never ship without one.
#[derive(Clone, IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: IconName,
    icon_size: IconSize,
    shape: IconButtonShape,
    style: ButtonStyle,
    size: ButtonSize,
    loading: bool,
    disabled: bool,
    tooltip: Tooltip,
    focus_handle: Option<FocusHandle>,
    tab_index: Option<isize>,
    on_click: Option<ClickHandler>,
    cursor_style: CursorStyle,
    hover_group: Option<SharedString>,
}

impl std::fmt::Debug for IconButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IconButton")
            .field("id", &self.id)
            .field("icon", &self.icon)
            .field("disabled", &self.disabled)
            .finish()
    }
}

impl IconButton {
    /// An icon button named by `tooltip`: shown on hover and read out as its label.
    pub fn new(id: impl Into<ElementId>, icon: IconName, tooltip: Tooltip) -> Self {
        Self {
            id: id.into(),
            icon,
            icon_size: IconSize::Small,
            shape: IconButtonShape::Square,
            style: ButtonStyle::Subtle,
            size: ButtonSize::Medium,
            loading: false,
            disabled: false,
            tooltip,
            focus_handle: None,
            tab_index: None,
            on_click: None,
            cursor_style: CursorStyle::PointingHand,
            hover_group: None,
        }
    }

    /// How big the glyph is.
    pub fn icon_size(mut self, size: IconSize) -> Self {
        self.icon_size = size;
        self
    }

    /// The box the glyph sits in.
    pub fn shape(mut self, shape: IconButtonShape) -> Self {
        self.shape = shape;
        self
    }

    /// How loud the button is.
    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    /// How big the box is.
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Show the button as working.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// The focus handle this button answers the keyboard with. With a handle, the handle
    /// itself carries the tab order: the button marks it a tab stop while it is enabled and
    /// applies [`Self::tab_index`] to it (GPUI ignores element-level `tab_stop`/`tab_index`
    /// when a handle is supplied).
    pub fn focus_handle(mut self, focus_handle: FocusHandle) -> Self {
        self.focus_handle = Some(focus_handle);
        self
    }

    /// Where this button sits in its container's tab order.
    pub fn tab_index(mut self, tab_index: isize) -> Self {
        self.tab_index = Some(tab_index);
        self
    }
}

impl Clickable for IconButton {
    fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    fn cursor_style(mut self, style: CursorStyle) -> Self {
        self.cursor_style = style;
        self
    }
}

impl Disableable for IconButton {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl VisibleOnHover for IconButton {
    fn visible_on_hover(mut self, group: SharedString) -> Self {
        self.hover_group = Some(group);
        self
    }
}

impl RenderOnce for IconButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let on_click = self.on_click.clone();
        let focus_handle = self.focus_handle.clone();
        let tooltip = self.tooltip.clone();
        let style = self.style;
        let loading = self.loading;
        let disabled = self.disabled || loading;
        let label = tooltip.title().clone();
        let glyph: gpui::AnyElement = if loading {
            spinner(self.icon_size)
        } else {
            Icon::new(self.icon)
                .size(self.icon_size)
                .color(if disabled {
                    Color::Disabled
                } else {
                    Color::Default
                })
                .into_any_element()
        };

        h_flex()
            .id(self.id)
            .font_ui(cx)
            .key_context(ACTIVATION_CONTEXT)
            .role(Role::Button)
            .aria_label(label)
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .size(
                self.size
                    .height()
                    .unwrap_or_else(|| px_to_rems(ICON_BUTTON_FALLBACK_PX)),
            )
            .map(|this| match self.shape {
                IconButtonShape::Square => this.rounded_sm(),
                IconButtonShape::Rounded => this.rounded_full(),
            })
            .border_1()
            .border_color(
                style
                    .border_color(cx)
                    .unwrap_or_else(|| cx.theme().colors().border_transparent),
            )
            .text_color(if disabled {
                Color::Disabled.color(cx)
            } else {
                style.text_color(cx)
            })
            .when_some(style.background(cx), |this, background| this.bg(background))
            .map(|this| match focus_handle.as_ref() {
                // With a caller's handle the handle's own tab_stop and tab_index decide
                // the tab order; the element-level ones only apply to handles GPUI creates.
                Some(handle) => {
                    let handle = handle.clone().tab_stop(!disabled);
                    let handle = match self.tab_index {
                        Some(index) => handle.tab_index(index),
                        None => handle,
                    };
                    this.track_focus(&handle)
                }
                None if on_click.is_some() => this.focusable().tab_stop(!disabled),
                None => this,
            })
            .focus_visible(|style_refinement| {
                style_refinement.border_color(cx.theme().colors().border_focused)
            })
            .when_some(self.tab_index, |this, index| this.tab_index(index))
            // No hover balloon while the tooltip says the control's own popover is
            // open (#265).
            .when(!tooltip.is_hidden(), |this| this.tooltip(tooltip.builder()))
            .map(|this| match &self.hover_group {
                Some(group) => this.invisible().group_hover(group.clone(), |s| s.visible()),
                None => this,
            })
            .child(glyph)
            .map(|this| {
                if disabled {
                    this.cursor_not_allowed().opacity(0.6)
                } else if style.fades_on_hover() {
                    this.cursor(self.cursor_style)
                        .hover(|s| s.opacity(0.88))
                        .active(|s| s.opacity(0.76))
                        .on_click(move |event, window, cx| {
                            if let Some(on_click) = &on_click {
                                cx.stop_propagation();
                                on_click(event, window, cx);
                            }
                        })
                } else {
                    this.cursor(self.cursor_style)
                        .hover(|s| s.bg(cx.theme().colors().ghost_element_hover))
                        .active(|s| s.bg(cx.theme().colors().ghost_element_active))
                        .on_click(move |event, window, cx| {
                            if let Some(on_click) = &on_click {
                                cx.stop_propagation();
                                on_click(event, window, cx);
                            }
                        })
                }
            })
    }
}

/// The box of an icon button whose [`ButtonSize`] says `None`.
const ICON_BUTTON_FALLBACK_PX: f32 = 24.;

/// The spinner shown while work is in flight: a pulsing loader glyph that inherits the
/// button's text color. GPUI's animation layer draws it still when the user asked for
/// reduced motion, so the label carries the message on its own.
pub(crate) fn spinner(size: IconSize) -> gpui::AnyElement {
    div()
        .with_animation(
            "ui-loading-spinner",
            gpui::Animation::new(std::time::Duration::from_millis(900))
                .repeat()
                .with_easing(gpui::ease_in_out),
            |element, delta| element.opacity(0.35 + 0.65 * delta),
        )
        .child(Icon::new(IconName::LoaderCircle).size(size))
        .into_any_element()
}

#[cfg(test)]
#[path = "button_tests.rs"]
mod tests;
