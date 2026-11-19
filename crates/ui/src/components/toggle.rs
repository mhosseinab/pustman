//! A control that is on, off, or half-on: the checkbox and the switch.
//!
//! The state is the design system's [`ToggleState`] — "select all" in a table header is
//! `Indeterminate` while some rows are ticked (REQ-UI-07: state is never carried by color
//! alone, so the control also draws a tick or a dash).

use std::rc::Rc;

use gpui::{
    App, Div, ElementId, FocusHandle, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    Role, SharedString, StatefulInteractiveElement, Styled, Toggled, Window, div,
    prelude::FluentBuilder,
};
use theme::ActiveTheme;

use crate::color::Color;
use crate::components::behavior::{Disableable, Toggleable};
use crate::components::label::{Label, LabelSize};
use crate::h_flex;
use crate::icons::{Icon, IconName, IconSize};
use crate::spacing::DynamicSpacing;
use crate::typography::StyledTypography;
use crate::units::px_to_rems;

/// The key context shared by controls that activate on Enter or Space; [`crate::init`]
/// binds the keys there.
pub(crate) const ACTIVATION_CONTEXT: &str = "Button";

/// How big the drawn control is, whatever its label's size.
const CONTROL_PX: f32 = 16.;
/// The switch's pill.
const SWITCH_WIDTH_PX: f32 = 32.;
const SWITCH_HEIGHT_PX: f32 = 18.;

/// What a toggle shows: on, off, or a mixed state for "some rows are ticked".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToggleState {
    /// On.
    Selected,
    /// Off.
    #[default]
    Unselected,
    /// Partly on: a parent of mixed children.
    Indeterminate,
}

impl From<bool> for ToggleState {
    fn from(selected: bool) -> Self {
        if selected {
            Self::Selected
        } else {
            Self::Unselected
        }
    }
}

impl ToggleState {
    /// Whether this counts as "on".
    pub fn is_selected(self) -> bool {
        self == Self::Selected
    }

    fn toggled(self) -> Toggled {
        match self {
            Self::Selected => Toggled::True,
            Self::Unselected => Toggled::False,
            Self::Indeterminate => Toggled::Mixed,
        }
    }

    /// The state after an activation: `Indeterminate` turns on.
    fn cycled(self) -> Self {
        match self {
            Self::Selected => Self::Unselected,
            Self::Unselected | Self::Indeterminate => Self::Selected,
        }
    }
}

/// Which shape the control draws as.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToggleKind {
    /// A square box with a tick: enabling a header row.
    #[default]
    Checkbox,
    /// A pill with a knob: an on/off setting.
    Switch,
}

type ChangeHandler = Rc<dyn Fn(ToggleState, &mut Window, &mut App)>;

/// A checkbox or switch. The label is what the control means — it is drawn next to the
/// control and doubles as the accessible name.
#[derive(Clone, IntoElement)]
pub struct Toggle {
    id: ElementId,
    label: SharedString,
    state: ToggleState,
    kind: ToggleKind,
    disabled: bool,
    color: Color,
    focus_handle: Option<FocusHandle>,
    on_change: Option<ChangeHandler>,
}

impl std::fmt::Debug for Toggle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Toggle")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("state", &self.state)
            .field("kind", &self.kind)
            .field("disabled", &self.disabled)
            .finish()
    }
}

impl Toggle {
    /// A toggle with `label` as its meaning and accessible name.
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        state: ToggleState,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            state,
            kind: ToggleKind::Checkbox,
            disabled: false,
            color: Color::Default,
            focus_handle: None,
            on_change: None,
        }
    }

    /// Draw it as a switch instead of a checkbox.
    pub fn kind(mut self, kind: ToggleKind) -> Self {
        self.kind = kind;
        self
    }

    /// Run `handler` with the state the user asked for. Enter and Space both activate.
    pub fn on_change(
        mut self,
        handler: impl Fn(ToggleState, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    /// Attach the focus handle this control answers the keyboard with, for callers that
    /// own a tab order. Without one the toggle keeps its own handle per [`ElementId`] and
    /// is still a tab stop that Enter and Space activate. With one, the handle itself
    /// carries the tab order: the toggle marks it a tab stop while it is enabled (GPUI
    /// ignores the element-level `tab_stop` when a handle is supplied).
    pub fn focus_handle(mut self, focus_handle: FocusHandle) -> Self {
        self.focus_handle = Some(focus_handle);
        self
    }

    /// Tint the label.
    pub fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    fn render_control(&self, cx: &App) -> Div {
        let colors = cx.theme().colors();
        let state = self.state;
        match self.kind {
            ToggleKind::Checkbox => {
                let (fill, border, icon) = if self.disabled {
                    (
                        colors.element_disabled,
                        colors.border_disabled,
                        Color::Disabled,
                    )
                } else {
                    match state {
                        ToggleState::Selected | ToggleState::Indeterminate => (
                            colors.accent_background,
                            colors.accent_background,
                            Color::Custom(colors.accent_foreground),
                        ),
                        ToggleState::Unselected => {
                            (colors.ghost_element_background, colors.border, self.color)
                        }
                    }
                };
                let symbol = match state {
                    ToggleState::Selected => Some(IconName::Check),
                    ToggleState::Indeterminate => Some(IconName::Minus),
                    ToggleState::Unselected => None,
                };
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px_to_rems(CONTROL_PX))
                    .rounded_xs()
                    .border_1()
                    .border_color(border)
                    .bg(fill)
                    .children(symbol.map(|name| Icon::new(name).size(IconSize::XSmall).color(icon)))
            }
            ToggleKind::Switch => {
                let (track, knob, border) = if self.disabled {
                    (
                        colors.element_disabled,
                        colors.border_disabled,
                        colors.border_disabled,
                    )
                } else {
                    match state {
                        ToggleState::Selected | ToggleState::Indeterminate => (
                            colors.accent_background,
                            colors.accent_foreground,
                            colors.accent_background,
                        ),
                        ToggleState::Unselected => {
                            (colors.element_background, colors.text_muted, colors.border)
                        }
                    }
                };
                div()
                    .flex()
                    .items_center()
                    .w(px_to_rems(SWITCH_WIDTH_PX))
                    .h(px_to_rems(SWITCH_HEIGHT_PX))
                    .rounded_full()
                    .border_1()
                    .border_color(border)
                    .bg(track)
                    .px(DynamicSpacing::Base01.rems(cx))
                    .when(state.is_selected(), |knob_row| knob_row.justify_end())
                    .child(
                        div()
                            .size(px_to_rems(SWITCH_HEIGHT_PX - 4.))
                            .rounded_full()
                            .bg(knob),
                    )
            }
        }
    }
}

impl RenderOnce for Toggle {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let label = self.label.clone();
        let state = self.state;
        let on_change = self.on_change.clone();
        let focus_handle = self.focus_handle.clone();
        let control = self.render_control(cx);
        let role = if self.kind == ToggleKind::Switch {
            Role::Switch
        } else {
            Role::CheckBox
        };

        h_flex()
            .id(self.id)
            .font_ui(cx)
            .key_context(ACTIVATION_CONTEXT)
            .role(role)
            .aria_label(label.clone())
            .aria_toggled(state.toggled())
            .gap(DynamicSpacing::Base02.rems(cx))
            .map(|this| match focus_handle.as_ref() {
                // With a caller's handle the handle's own tab_stop decides the tab order;
                // the element-level one only applies to handles GPUI creates.
                Some(handle) => this.track_focus(&handle.clone().tab_stop(!self.disabled)),
                None if on_change.is_some() => this.focusable().tab_stop(!self.disabled),
                None => this,
            })
            .focus_visible(|style| style.border_color(cx.theme().colors().border_focused))
            .child(control)
            .child(Label::new(label).size(LabelSize::Small).color(self.color))
            .map(|this| {
                if self.disabled {
                    this.cursor_not_allowed().opacity(0.6)
                } else {
                    this.cursor_pointer().on_click(move |_event, window, cx| {
                        if let Some(on_change) = &on_change {
                            cx.stop_propagation();
                            on_change(state.cycled(), window, cx);
                        }
                    })
                }
            })
    }
}

impl Toggleable for Toggle {
    fn toggle_state(mut self, state: impl Into<ToggleState>) -> Self {
        self.state = state.into();
        self
    }
}

impl Disableable for Toggle {
    fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}
