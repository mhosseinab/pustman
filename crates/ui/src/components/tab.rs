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

use crate::components::behavior::{Clickable, Disableable, Toggleable};
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

    /// Whether this tab is disabled: it then offers nothing to click — no activation and
    /// no close button (see [`Disableable`]).
    pub(crate) fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// The tab's title, for tooltips and tab lists.
    pub fn label(&self) -> &SharedString {
        &self.label
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
        // Test hooks: `VisualTestContext::debug_bounds` finds this tab and its close
        // button under these keys.
        let selector = self.id.to_string();
        let close_selector = format!("{selector}-close");
        let tooltip_text = if self.dirty {
            SharedString::from(format!("{label} — unsaved changes"))
        } else {
            label.clone()
        };
        let colors = cx.theme().colors();
        let active = self.active;
        let disabled = self.disabled;
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
            .font_ui(cx)
            .role(Role::Tab)
            .aria_label(tooltip_text.clone())
            .aria_selected(active)
            .items_center()
            .flex_shrink_0()
            .gap(DynamicSpacing::Base02.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .when_some(self.width, |this, width| this.w(width))
            .tooltip(Tooltip::text(tooltip_text.clone()).builder())
            .map(|this| {
                if active {
                    this.bg(colors.tab_active_background)
                        .text_color(colors.text_accent)
                } else {
                    this.bg(colors.tab_inactive_background)
                        .text_color(colors.text_muted)
                }
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
                            .size(px_to_rems(DIRTY_DOT_PX))
                            .rounded_full()
                            .bg(colors.text_accent),
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
    }
}
