//! A tab in a [`crate::TabBar`]: one open request.
//!
//! The tab is a value — the bar owns the selection *and* activation — so a tab can be
//! built once and stamped active or inactive by whoever lays the row out, and a click on
//! it reaches the same [`crate::TabBar`] path the keyboard does (`components/behavior.rs`).
//! It carries the three states REQ-UI-02 asks for: active, dirty (the dot) and pinned.

use gpui::{
    App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce, Role,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
};
use theme::ActiveTheme;

use crate::components::behavior::{Clickable, Disableable, Toggleable, VisibleOnHover};
use crate::components::button::{ClickHandler, IconButton};
use crate::components::label::{Label, LabelSize};
use crate::components::toggle::ToggleState;
use crate::components::tooltip::Tooltip;
use crate::h_flex;
use crate::icons::{Icon, IconName, IconSize};
use crate::spacing::DynamicSpacing;
use crate::typography::StyledTypography;
use crate::units::px_to_rems;

/// The dot that says "this tab has unsaved changes".
const DIRTY_DOT_PX: f32 = 6.;

/// The accent line under the active section tab.
const UNDERLINE_PX: f32 = 2.;

/// The height of one chip in a segmented bar: compact enough that the switch reads as a
/// control inside the content, not as another row of tabs.
const SEGMENT_HEIGHT_PX: f32 = 22.;

/// How a [`crate::TabBar`] draws its tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TabStyle {
    /// Open requests across a pane: filled boxes on a band.
    #[default]
    Pane,
    /// Sections of an editor: transparent, underlined when active.
    Section,
    /// A second-level switch: chips in one rounded track, the active chip raised.
    Segmented,
}

/// One tab: a title, an optional icon, and the state a request tab can be in.
#[derive(Clone, IntoElement)]
pub struct Tab {
    id: ElementId,
    label: SharedString,
    icon: Option<IconName>,
    active: bool,
    dirty: bool,
    pinned: bool,
    disabled: bool,
    style: TabStyle,
    width: Option<gpui::Rems>,
    on_activated: Option<ClickHandler>,
    on_close: Option<ClickHandler>,
}

impl std::fmt::Debug for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tab")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("active", &self.active)
            .field("dirty", &self.dirty)
            .field("pinned", &self.pinned)
            .finish()
    }
}

impl Tab {
    /// A tab with no icon and no close button.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            active: false,
            dirty: false,
            pinned: false,
            disabled: false,
            style: TabStyle::Pane,
            width: None,
            on_activated: None,
            on_close: None,
        }
    }

    /// A method or kind glyph before the title.
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Show the unsaved-changes dot. The dot never carries the meaning alone — the
    /// tab's tooltip says the tab is unsaved.
    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }

    /// Pin the tab: pinned tabs are not closed by "close the others".
    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// Pin this tab to a width (pinned tabs are narrow).
    pub fn width(mut self, width: gpui::Rems) -> Self {
        self.width = Some(width);
        self
    }

    /// Show a close button that runs `handler`. Without it the tab has no close button,
    /// and a disabled tab renders none at all (see [`Disableable`]).
    pub fn on_close(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_close = Some(std::rc::Rc::new(handler));
        self
    }

    /// Run `handler` when the tab itself is activated. Only the [`crate::TabBar`] calls
    /// this: the bar routes every activation — a mouse click here and Enter/Space on the
    /// bar — through one path, so the two can never disagree (`components/behavior.rs`)
    /// and the tab carries no public `on_click` of its own.
    pub(crate) fn on_activated(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_activated = Some(std::rc::Rc::new(handler));
        self
    }

    /// Draw in the bar's style ([`crate::TabBar::section`], [`crate::TabBar::segmented`]).
    pub(crate) fn style(mut self, style: TabStyle) -> Self {
        self.style = style;
        self
    }

    /// Whether this tab is disabled: it then offers nothing to click — no activation and
    /// no close button (see [`Disableable`]).
    pub(crate) fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// The tab's title, for tooltips and tab lists.
    pub fn label(&self) -> &SharedString {
        &self.label
    }

    /// The tab's identity in the frame: the root of the selectors its chrome is found
    /// under ([`gpui::VisualTestContext::debug_bounds`]).
    pub(crate) fn selector(&self) -> String {
        self.id.to_string()
    }
}

impl Toggleable for Tab {
    fn toggle_state(mut self, state: impl Into<ToggleState>) -> Self {
        self.active = state.into().is_selected();
        self
    }
}

impl Disableable for Tab {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for Tab {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let label = self.label.clone();
        // Test hooks: `VisualTestContext::debug_bounds` finds this tab and its chrome
        // under these keys.
        let selector = self.selector();
        let close_selector = format!("{selector}-close");
        let dot_selector = format!("{selector}-dot");
        let underline_selector = format!("{selector}-underline");
        // The row hover that swaps the trailing slot's two glyphs: a dirty tab shows its
        // dot at rest and its close × while the pointer rests on the tab (#214).
        let hover_group = SharedString::from(format!("{selector}-hover"));
        let tooltip_text = if self.dirty {
            SharedString::from(format!("{label} — unsaved changes"))
        } else {
            label.clone()
        };
        let colors = cx.theme().colors();
        let active = self.active;
        let disabled = self.disabled;
        let style = self.style;
        let on_activated = self.on_activated;

        // A disabled tab offers nothing to click: no close button (the `Disableable`
        // contract is "fires no handler" — see `components/behavior.rs`).
        let close_button = if disabled {
            None
        } else {
            self.on_close.as_ref().map(|on_close| {
                div().debug_selector(move || close_selector.clone()).child(
                    IconButton::new(
                        (self.id.clone(), "close"),
                        IconName::X,
                        Tooltip::text(format!("Close {label}")),
                    )
                    .size(crate::components::button::ButtonSize::Compact)
                    .icon_size(IconSize::XSmall)
                    // The × is drawn only where the strip offers it: on the active tab,
                    // or where the pointer rests (#214). On a dirty tab the dot has the
                    // slot at rest, so there the × is out of the frame until hover swaps
                    // them; a clean tab's slot keeps its hit target while its × is faded
                    // out, so a click that lands before the reveal still closes.
                    .map(|button| match (self.dirty, active) {
                        (true, _) => button.visible_on_hover(hover_group.clone()),
                        (false, true) => button,
                        (false, false) => button.faded_until_hovered(hover_group.clone()),
                    })
                    .on_click({
                        let on_close = on_close.clone();
                        move |event, window, cx| on_close(event, window, cx)
                    }),
                )
            })
        };

        h_flex()
            .id(self.id)
            .debug_selector(|| selector)
            .group(hover_group.clone())
            .font_ui(cx)
            .role(Role::Tab)
            .aria_label(tooltip_text.clone())
            .aria_selected(active)
            .items_center()
            .flex_shrink_0()
            .gap(DynamicSpacing::Base02.rems(cx))
            .map(|this| match style {
                TabStyle::Pane => this.px(DynamicSpacing::Base04.rems(cx)),
                TabStyle::Section => this.relative().px(DynamicSpacing::Base12.rems(cx)),
                TabStyle::Segmented => this
                    .h(px_to_rems(SEGMENT_HEIGHT_PX))
                    .px(DynamicSpacing::Base08.rems(cx))
                    .rounded_sm()
                    .border_1(),
            })
            .when_some(self.width, |this, width| this.w(width))
            // A tooltip only when it adds what the tab does not show: the dirty state, or
            // a label a fixed width may truncate. Repeating a visible label covers the bar.
            .when(self.dirty || self.width.is_some(), |this| {
                this.tooltip(Tooltip::text(tooltip_text.clone()).builder())
            })
            .map(|this| match (style, active) {
                (TabStyle::Section, true) => this.text_color(colors.text),
                (TabStyle::Section, false) => this.text_color(colors.text_muted),
                (TabStyle::Segmented, true) => this
                    .bg(colors.elevated_surface_background)
                    .border_color(colors.border)
                    .text_color(colors.text),
                (TabStyle::Segmented, false) => this
                    .border_color(colors.border_transparent)
                    .text_color(colors.text_muted),
                (TabStyle::Pane, true) => this
                    .bg(colors.tab_active_background)
                    .text_color(colors.text_accent),
                (TabStyle::Pane, false) => this
                    .bg(colors.tab_inactive_background)
                    .text_color(colors.text_muted),
            })
            .when(disabled, |this| this.opacity(0.5))
            .hover(|style| {
                if active {
                    style
                } else {
                    style.bg(colors.ghost_element_hover)
                }
            })
            .map(|this| {
                if disabled {
                    this
                } else if let Some(on_activated) = on_activated {
                    this.cursor_pointer().on_click(move |event, window, cx| {
                        cx.stop_propagation();
                        on_activated(event, window, cx);
                    })
                } else {
                    this
                }
            })
            .children(self.icon.map(|icon| Icon::new(icon).size(IconSize::Small)))
            .child(Label::new(label).size(LabelSize::Small).truncate())
            .map(|this| {
                if self.dirty {
                    this.child(
                        div()
                            .debug_selector(move || dot_selector)
                            .size(px_to_rems(DIRTY_DOT_PX))
                            .rounded_full()
                            .bg(colors.text_accent)
                            .map(|this| {
                                if self.on_close.is_some() && !disabled {
                                    // The close × takes the dot's place while the tab is
                                    // hovered (#214): one glyph at a time, never both.
                                    this.group_hover(hover_group.clone(), |style| style.invisible())
                                } else {
                                    this
                                }
                            }),
                    )
                } else {
                    this
                }
            })
            .children(
                self.pinned
                    .then(|| Icon::new(IconName::Pin).size(IconSize::XSmall)),
            )
            .children(close_button)
            .when(style == TabStyle::Section && active, |this| {
                this.child(
                    div()
                        .debug_selector(move || underline_selector)
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .right_0()
                        .h(px_to_rems(UNDERLINE_PX))
                        .bg(colors.border_selected),
                )
            })
    }
}

#[cfg(test)]
#[path = "tab_tooltip_tests.rs"]
mod tooltip_tests;
