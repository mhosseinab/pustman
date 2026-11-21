//! The row of tabs across the top of a pane.
//!
//! The bar owns the selection and the focus handle, and answers Left/Right to cycle and
//! Enter/Space to activate — a tab bar is one control with a row of labels, not a row of
//! independent buttons (REQ-UI-02, REQ-UI-07).

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, ClickEvent, Context, ElementId, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled,
    Window, div,
};
use theme::ActiveTheme;

use crate::actions::{Activate, SelectNext, SelectPrevious};
use crate::components::behavior::Toggleable;
use crate::components::tab::{Tab, TabStyle};
use crate::h_flex;
use crate::spacing::DynamicSpacing;
use crate::typography::StyledTypography;
use crate::units::px_to_rems;

/// The key context the tab bar answers the keyboard in; [`crate::init`] binds the arrows.
pub(crate) const TAB_BAR_CONTEXT: &str = "TabBar";

/// The height of the request tab row: the redesign pack's `layout.requestTabs`, 40 px.
const TAB_BAR_HEIGHT_PX: f32 = 40.;

/// The hairline between two tabs the strip does not highlight (#214).
const TAB_SEPARATOR_PX: f32 = 1.;

/// Test hook for the rounded track a segmented bar draws its chips in.
pub const SEGMENTED_TRACK_SELECTOR: &str = "tab-bar-segmented-track";

/// What a [`TabBar`] tells its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabBarEvent {
    /// The selection moved — with the arrows, that also switches to the tab. Clicking a
    /// tab moves the selection too, but announces [`TabBarEvent::Activated`] instead.
    Selected { index: usize },
    /// The tab was activated: clicked with the mouse, or confirmed with Enter or Space.
    /// Both run the same [`TabBar`] path (`components/behavior.rs`), so they can never
    /// disagree — one event per activation, never two.
    Activated { index: usize },
}

impl EventEmitter<TabBarEvent> for TabBar {}

/// A row of [`Tab`]s with one selection and one focus target.
pub struct TabBar {
    focus_handle: FocusHandle,
    tabs: Vec<Tab>,
    selected: usize,
    style: TabStyle,
}

impl TabBar {
    /// An empty bar. Fill it with [`Self::with_tabs`] or [`Self::set_tabs`].
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            // The bar is one control, so it must be a tab stop — `FocusHandle::new`
            // defaults `tab_stop` to false, and Tab would never land on the bar.
            focus_handle: cx.focus_handle().tab_stop(true),
            tabs: Vec::new(),
            selected: 0,
            style: TabStyle::Pane,
        }
    }

    /// Draw the bar as section tabs inside an editor (Params…Settings, Body/Headers…):
    /// no band behind them, room around each label, and an accent underline under the
    /// active one (pack `workbench_blueprint.svg`). Pane tabs keep the default look.
    pub fn section(mut self) -> Self {
        self.style = TabStyle::Section;
        self
    }

    /// Draw the bar as a segmented switch: a second level of tabs under a section row
    /// (Pretty/Raw/Hex/Preview) reads as one compact control, its chips in a rounded
    /// track, rather than as another underlined row stacked under the first.
    pub fn segmented(mut self) -> Self {
        self.style = TabStyle::Segmented;
        self
    }

    /// The tabs this bar shows, before it is put on screen.
    pub fn with_tabs(mut self, tabs: Vec<Tab>) -> Self {
        self.selected = self.selected.min(tabs.len().saturating_sub(1));
        self.tabs = tabs;
        self
    }

    /// Replace the tabs and keep the selection in range.
    pub fn set_tabs(&mut self, tabs: Vec<Tab>, cx: &mut Context<Self>) {
        self.tabs = tabs;
        self.selected = self.selected.min(self.tabs.len().saturating_sub(1));
        cx.notify();
    }

    /// The index of the selected tab.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// How many tabs the bar has.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether the bar has no tabs at all.
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Select `index`, clamped to the tabs the bar has.
    pub fn set_selected(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            self.selected = 0;
        } else {
            self.selected = index.min(self.tabs.len() - 1);
        }
        cx.emit(TabBarEvent::Selected {
            index: self.selected,
        });
        cx.notify();
    }

    /// Move keyboard focus to the bar.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.focus_handle.focus(window, cx);
    }

    fn select_next(&mut self, _: &SelectNext, _window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        self.set_selected((self.selected + 1) % self.tabs.len(), cx);
    }

    fn select_previous(
        &mut self,
        _: &SelectPrevious,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.is_empty() {
            return;
        }
        self.set_selected((self.selected + self.tabs.len() - 1) % self.tabs.len(), cx);
    }

    fn activate(&mut self, _: &Activate, window: &mut Window, cx: &mut Context<Self>) {
        // Enter and Space also match when a descendant control (a tab's close button) has
        // focus, because the bar's key context stays on the stack. Those keys belong to
        // the focused control — GPUI synthesizes a click on it — so pass them on instead of
        // confirming the selection from underneath it.
        if !self.focus_handle.is_focused(window) {
            cx.propagate();
            return;
        }
        self.activate_tab(self.selected, cx);
    }

    /// Activate the tab at `index`: the single path a tab is activated by, shared by the
    /// mouse (each tab's click) and the keyboard ([`Self::activate`]), so the two can
    /// never disagree about what activation does (`components/behavior.rs`). A disabled
    /// tab is never activated.
    fn activate_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.tabs.get(index).is_none_or(Tab::is_disabled) {
            return;
        }
        // The activated tab is the bar's selection from here on; the owner hears about it
        // through `Activated`, which carries the index, so `Selected` stays the arrows'
        // event.
        self.selected = index;
        cx.emit(TabBarEvent::Activated { index });
        cx.notify();
    }
}

impl Focusable for TabBar {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TabBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected;
        let focus_handle = self.focus_handle.clone();
        let style = self.style;
        let colors = cx.theme().colors();
        let mut children: Vec<AnyElement> = Vec::with_capacity(self.tabs.len() * 2);
        for (index, tab) in self.tabs.iter().enumerate() {
            // Between two tabs the strip does not highlight, a hairline marks where one
            // ends and the next begins (#214). Beside the active tab the highlight's own
            // edge already delimits it, and the strip starts without one.
            if style == TabStyle::Pane && index > 0 && index != selected && index - 1 != selected {
                children.push(
                    div()
                        .debug_selector({
                            let selector = format!("{}-separator", tab.selector());
                            move || selector
                        })
                        .w(px_to_rems(TAB_SEPARATOR_PX))
                        .flex_none()
                        .my(DynamicSpacing::Base08.rems(cx))
                        .bg(colors.border_variant)
                        .into_any_element(),
                );
            }
            children.push(
                tab.clone()
                    .style(style)
                    .toggle_state(index == selected)
                    .on_activated(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        this.activate_tab(index, cx);
                    }))
                    .into_any_element(),
            );
        }
        h_flex()
            .id(ElementId::from(&focus_handle))
            .font_ui(cx)
            .key_context(TAB_BAR_CONTEXT)
            .role(gpui::Role::TabList)
            .map(|this| match style {
                TabStyle::Pane => this
                    .h(px_to_rems(TAB_BAR_HEIGHT_PX))
                    .items_stretch()
                    .bg(colors.tab_bar_background),
                TabStyle::Section => this.h(px_to_rems(TAB_BAR_HEIGHT_PX)).items_stretch(),
                TabStyle::Segmented => this
                    .debug_selector(|| SEGMENTED_TRACK_SELECTOR.to_owned())
                    .flex_none()
                    .items_center()
                    .gap(DynamicSpacing::Base02.rems(cx))
                    .p(DynamicSpacing::Base02.rems(cx))
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_variant)
                    .bg(colors.element_background),
            })
            .track_focus(&focus_handle)
            .focus_visible(|focused| focused.border_color(colors.border_focused))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::activate))
            .children(children)
    }
}

#[cfg(test)]
#[path = "tab_bar_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tab_bar_keyboard_tests.rs"]
mod keyboard_tests;

#[cfg(test)]
#[path = "tab_bar_section_tests.rs"]
mod section_tests;

#[cfg(test)]
#[path = "tab_bar_segmented_tests.rs"]
mod segmented_tests;

#[cfg(test)]
#[path = "tab_bar_chrome_tests.rs"]
mod chrome_tests;
