//! The right-click menu: the short list of actions for the thing under the pointer.
//!
//! A menu is one control — it owns the highlight and the focus handle — and answers the
//! keyboard in the [`MENU_CONTEXT`] key context with the component actions every
//! composite control uses ([`crate::actions`] — the same ones the [`crate::TabBar`]
//! uses): Up and Down move the highlight, Enter runs the highlighted entry, Escape closes
//! the menu without running anything. The mouse and the keyboard go through one
//! activation path ([`ContextMenu::activate_entry`]), so they can never disagree about
//! what an entry does (`components/behavior.rs`).

use std::rc::Rc;

use gpui::{
    Anchor, AnyElement, App, ClickEvent, Context, ElementId, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement, IntoElement, ParentElement, Rems, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, prelude::FluentBuilder,
};
use theme::ActiveTheme;

use crate::actions::{Activate, Cancel, SelectNext, SelectPrevious};
use crate::color::Color;
use crate::components::button::ClickHandler;
use crate::components::divider::Divider;
use crate::components::label::{Label, LabelSize};
use crate::elevation::StyledExt;
use crate::h_flex;
use crate::spacing::DynamicSpacing;
use crate::typography::StyledTypography;
use crate::units::px_to_rems;
use crate::v_flex;

/// The key context the menu answers the keyboard in; [`crate::init`] binds the arrows,
/// Enter and Escape.
pub(crate) const MENU_CONTEXT: &str = "Menu";

/// Test hook: the menu container itself — what the floating layer anchors and snaps
/// (`VisualTestContext::debug_bounds`, the entries' key hooks below).
pub const MENU_SELECTOR: &str = "context-menu";

/// Test hook: an entry's hint sits under its key with this suffix
/// (`"Save"` → `"Save-hint"`).
pub const HINT_SUFFIX: &str = "-hint";

/// The narrowest a menu grows, so a two-word entry never wraps.
const MIN_WIDTH_PX: f32 = 160.;

/// The height of one menu entry at the default scale.
const ENTRY_HEIGHT_PX: f32 = 24.;

/// What a [`ContextMenu`] tells its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextMenuEvent {
    /// The menu is done — Escape, or an entry ran. The owner hides or drops the menu.
    Dismissed,
}

/// One line of a [`ContextMenu`]: an entry the user can run, or a separator between
/// groups of entries.
enum MenuItem {
    Entry {
        /// The element id: the label, deduplicated when the menu is built — never the
        /// line's position, which shifts as entries come and go.
        key: SharedString,
        label: SharedString,
        /// A label colour of the caller's choosing (a method verb). [`None`] draws the
        /// row's own text color.
        color: Option<Color>,
        /// Muted text at the row's end — an application menu's key binding, or the `▸`
        /// of an entry that opens a submenu. [`None`] draws nothing there.
        hint: Option<SharedString>,
        disabled: bool,
        on_click: Option<ClickHandler>,
    },
    Separator {
        key: SharedString,
    },
}

impl MenuItem {
    fn key(&self) -> &SharedString {
        match self {
            Self::Entry { key, .. } | Self::Separator { key } => key,
        }
    }

    /// Separators and disabled entries are shown but can never run, so the keyboard steps
    /// over them and they cannot be the highlight.
    fn is_enabled_entry(&self) -> bool {
        match self {
            Self::Entry { disabled, .. } => !disabled,
            Self::Separator { .. } => false,
        }
    }

    fn render(
        &self,
        selected: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> AnyElement {
        match self {
            Self::Entry {
                key,
                label,
                color,
                hint,
                disabled,
                ..
            } => {
                let disabled = *disabled;
                let colors = cx.theme().colors();
                h_flex()
                    .id(key.clone())
                    // Test hooks: `VisualTestContext::debug_bounds` finds this entry under
                    // its key (same pattern as `components/tab.rs`).
                    .debug_selector(|| key.to_string())
                    .role(Role::MenuItem)
                    .aria_label(label.clone())
                    .aria_selected(selected)
                    .min_h(px_to_rems(ENTRY_HEIGHT_PX))
                    .mx(DynamicSpacing::Base02.rems(cx))
                    .px(DynamicSpacing::Base04.rems(cx))
                    .gap(DynamicSpacing::Base02.rems(cx))
                    .rounded_sm()
                    .text_color(if disabled {
                        Color::Disabled.color(cx)
                    } else if selected {
                        colors.text_accent
                    } else {
                        colors.text
                    })
                    .when(selected, |this| this.bg(colors.element_selected))
                    .child(div().flex_1().min_w_0().child(
                        Label::new(label.clone()).truncate().map(|label| {
                            // The entry's color is its identity (a method verb), not its
                            // state: the highlight still marks the selection, by fill.
                            match color {
                                Some(color) => label.color(*color),
                                None => label,
                            }
                        }),
                    ))
                    .when_some(hint.clone(), |row, hint| {
                        let hint_key = format!("{key}{HINT_SUFFIX}");
                        row.child(
                            div()
                                .debug_selector(move || hint_key.clone())
                                .flex_none()
                                .pl(DynamicSpacing::Base08.rems(cx))
                                .child(Label::new(hint).size(LabelSize::XSmall).color(
                                    if disabled {
                                        Color::Disabled
                                    } else {
                                        Color::Muted
                                    },
                                )),
                        )
                    })
                    .map(|this| {
                        if disabled {
                            this.cursor_not_allowed().opacity(0.6)
                        } else {
                            this.cursor_pointer()
                                .hover(|style| style.bg(colors.ghost_element_hover))
                                .on_click(on_click)
                        }
                    })
                    .into_any_element()
            }
            Self::Separator { key } => div()
                .id(key.clone())
                .debug_selector(|| key.to_string())
                .my(DynamicSpacing::Base02.rems(cx))
                .mx(DynamicSpacing::Base02.rems(cx))
                .child(Divider::horizontal())
                .into_any_element(),
        }
    }
}

/// A keyboard-navigable menu of entries, opened where the user asked for it (right-click
/// on a row, a dropdown trigger) and dismissed by Escape or by running an entry.
///
/// Build it in the owner's `cx` — `cx.new(|cx| ContextMenu::new(cx).entry(…))` — keep the
/// [`gpui::Entity`], and subscribe to [`ContextMenuEvent::Dismissed`] to know when to
/// hide it. Focus the menu ([`Self::focus`]) when it opens: it is one tab stop, and the
/// whole keyboard lives on it.
pub struct ContextMenu {
    focus_handle: FocusHandle,
    items: Vec<MenuItem>,
    selected: usize,
}

impl ContextMenu {
    /// An empty menu. Fill it with [`Self::entry`], [`Self::disabled_entry`] and
    /// [`Self::separator`] before showing it.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            // The menu is one control, so it must be a tab stop — `FocusHandle::new`
            // defaults `tab_stop` to false (`components/tab_bar.rs`).
            focus_handle: cx.focus_handle().tab_stop(true),
            items: Vec::new(),
            selected: 0,
        }
    }

    /// An entry `on_click` runs — one handler for the mouse and the keyboard, so the two
    /// can never disagree. Handlers must not re-enter this entity: they run inside its
    /// own action dispatch.
    pub fn entry(
        mut self,
        label: impl Into<SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let label = label.into();
        let key = self.unique_key(&label);
        self.items.push(MenuItem::Entry {
            key,
            label,
            color: None,
            hint: None,
            disabled: false,
            on_click: Some(Rc::new(on_click)),
        });
        // The highlight starts on the first entry that can be run.
        self.selected = self.first_enabled_entry();
        self
    }

    /// An entry whose label draws in `color` — the label's identity, like a method verb
    /// (the theme's method tokens), never its state: selection still marks the row by
    /// fill. One handler for the mouse and the keyboard, like [`Self::entry`].
    pub fn colored_entry(
        mut self,
        label: impl Into<SharedString>,
        color: Color,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        let label = label.into();
        let key = self.unique_key(&label);
        self.items.push(MenuItem::Entry {
            key,
            label,
            color: Some(color),
            hint: None,
            disabled: false,
            on_click: Some(Rc::new(on_click)),
        });
        // The highlight starts on the first entry that can be run.
        self.selected = self.first_enabled_entry();
        self
    }

    /// An entry that is shown but can never run — it is grayed out and the keyboard steps
    /// over it.
    pub fn disabled_entry(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        let key = self.unique_key(&label);
        self.items.push(MenuItem::Entry {
            key,
            label,
            color: None,
            hint: None,
            disabled: true,
            on_click: None,
        });
        self
    }

    /// Give the entry added last a muted `hint` at its end: the key binding an
    /// application menu shows beside an entry, or the `▸` of an entry that opens a
    /// submenu. A separator takes no hint, so after one this does nothing.
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        if let Some(MenuItem::Entry { hint: slot, .. }) = self.items.last_mut() {
            *slot = Some(hint.into());
        }
        self
    }

    /// A line between groups of entries. It is never selectable.
    pub fn separator(mut self) -> Self {
        let key = self.unique_key("separator");
        self.items.push(MenuItem::Separator { key });
        self
    }

    /// The index of the highlighted line — entry or separator — in the menu.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// How many lines — entries and separators — the menu shows.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the menu has no lines at all.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Move keyboard focus to the menu. The owner does this where the menu opens.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.focus_handle.focus(window, cx);
    }

    fn first_enabled_entry(&self) -> usize {
        self.items
            .iter()
            .position(MenuItem::is_enabled_entry)
            .unwrap_or(0)
    }

    /// The element id for a new line: its `base` name, deduplicated if an earlier line
    /// took it already.
    fn unique_key(&self, base: &str) -> SharedString {
        let mut key = SharedString::from(base);
        let mut duplicate = 2;
        while self.items.iter().any(|item| item.key() == &key) {
            key = SharedString::from(format!("{base}-{duplicate}"));
            duplicate += 1;
        }
        key
    }

    /// Move the highlight to the next runnable entry, wrapping around the ends of the
    /// menu so the arrows never fall off it.
    fn move_selection(&mut self, forward: bool, cx: &mut Context<Self>) {
        let item_count = self.items.len();
        for step in 1..=item_count {
            let candidate = if forward {
                (self.selected + step) % item_count
            } else {
                (self.selected + item_count - step) % item_count
            };
            if self
                .items
                .get(candidate)
                .is_some_and(MenuItem::is_enabled_entry)
            {
                self.selected = candidate;
                cx.notify();
                return;
            }
        }
    }

    fn select_next(&mut self, _: &SelectNext, _window: &mut Window, cx: &mut Context<Self>) {
        self.move_selection(true, cx);
    }

    fn select_previous(
        &mut self,
        _: &SelectPrevious,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.move_selection(false, cx);
    }

    fn activate(&mut self, _: &Activate, window: &mut Window, cx: &mut Context<Self>) {
        // Enter and Space also match when a descendant of the menu has focus, because the
        // menu's key context stays on the stack. Those keys belong to the focused control
        // — GPUI synthesizes a click on it — so pass them on instead of running the
        // highlight from underneath it (`components/tab_bar.rs`).
        if !self.focus_handle.is_focused(window) {
            cx.propagate();
            return;
        }
        self.activate_entry(self.selected, &ClickEvent::default(), window, cx);
    }

    fn cancel(&mut self, _: &Cancel, _window: &mut Window, cx: &mut Context<Self>) {
        cx.emit(ContextMenuEvent::Dismissed);
    }

    /// Run line `index` — the one path a menu entry is activated by, shared by the mouse
    /// (each entry's click) and the keyboard ([`Self::activate`]) — and tell the owner
    /// the menu is done. A separator or a disabled entry runs nothing and leaves the menu
    /// open.
    fn activate_entry(
        &mut self,
        index: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on_click = match self.items.get(index) {
            Some(MenuItem::Entry {
                disabled: false,
                on_click,
                ..
            }) => on_click.clone(),
            _ => return,
        };
        if let Some(on_click) = on_click {
            on_click(event, window, cx);
        }
        cx.emit(ContextMenuEvent::Dismissed);
    }
}

impl Focusable for ContextMenu {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<ContextMenuEvent> for ContextMenu {}

impl Render for ContextMenu {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected;
        let focus_handle = self.focus_handle.clone();
        v_flex()
            .id(ElementId::from(&focus_handle))
            .debug_selector(|| String::from(MENU_SELECTOR))
            .key_context(MENU_CONTEXT)
            .role(Role::Menu)
            .font_ui(cx)
            .track_focus(&focus_handle)
            .focus_visible(|style| style.border_color(cx.theme().colors().border_focused))
            .elevation_2(cx)
            // The menu owns the pointer while open: without it, hover and tooltips of the
            // controls drawn underneath still fire through the menu (#205).
            .occlude()
            .min_w(px_to_rems(MIN_WIDTH_PX))
            .py(DynamicSpacing::Base02.rems(cx))
            .rounded_sm()
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::activate))
            .on_action(cx.listener(Self::cancel))
            .children(self.items.iter().enumerate().map(|(index, item)| {
                item.render(
                    index == selected,
                    cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.activate_entry(index, event, window, cx);
                    }),
                    cx,
                )
            }))
    }
}

/// A picker menu floating under its trigger — the one shape every
/// [`crate::components::button::Button`] + [`ContextMenu`] dropdown uses (`url_bar`'s
/// method picker and the request editor's mode, language, auth-type, key-location and
/// HTTP-version pickers): anchored just under the trigger so the editor below cannot
/// clip it, snapped to the window edge so it never leaves the window. `trigger_height`
/// is the trigger's height — the offset the menu drops below it.
pub fn floating_menu(
    menu: &Option<Entity<ContextMenu>>,
    trigger_height: Rems,
    cx: &App,
) -> Option<AnyElement> {
    menu.clone().map(|menu| {
        div()
            .absolute()
            .top(trigger_height)
            .left_0()
            .child(
                deferred(
                    anchored()
                        .anchor(Anchor::TopLeft)
                        .snap_to_window_with_margin(DynamicSpacing::Base04.px(cx))
                        .child(menu),
                )
                .with_priority(1),
            )
            .into_any_element()
    })
}

#[cfg(test)]
#[path = "context_menu_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "context_menu_occlusion_tests.rs"]
mod occlusion_tests;

#[cfg(test)]
#[path = "context_menu_floating_tests.rs"]
mod floating_menu_tests;

#[cfg(test)]
#[path = "context_menu_hint_tests.rs"]
mod hint_tests;
