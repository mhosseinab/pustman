//! `{{variable}}` autocomplete behind the request fields (PRD REQ-VAR-04): typing `{{`
//! in a field opens a popover with the variable names of every scope in effect, and
//! choosing one — the arrows and Enter, or a click — inserts it closed with `}}`.
//!
//! The trigger, the replace range and the insert are [`text_input::completion`]'s hook;
//! this module owns what the popover shows: the names across [`ScopeStack`]'s scopes
//! (names only — a value is never read here; the hover that shows one is S33c), the
//! filtering and the keyboard contract, and the anchored [`VirtualList`] surface under
//! the field.
//!
//! `Decision:` the popover leaves focus in the field — typing keeps narrowing the list —
//! so it is the field's passive overlay rather than an [`ui::ContextMenu`], which is one
//! focusable control whose whole keyboard lives on its own focus handle and whose
//! selection cannot be moved from outside. The surface copies the menu's chrome, and the
//! rows follow `command_palette::picker`'s match-row shape (a child id of the row's
//! stable key, so a click inserts *this* name even if the list re-orders underneath the
//! pointer).
//!
//! `Decision:` the keys are bound in [`VariableAutocomplete::new`] — the
//! `history_panel`'s shape for a crate whose `init` a step may not edit — reusing `ui`'s
//! component actions under this module's [`VARIABLE_COMPLETION_CONTEXT`]. Enter arrives
//! as the focused field's [`TextInputEvent::Submitted`] and Escape as
//! `pustman_actions::request::Cancel` (the URL bar's "back out of what is open"
//! contract), so neither needs a binding of its own.
//!
//! `Decision:` only the URL bar carries the popover in this step (its `url_bar` module
//! attaches [`VariableAutocomplete`] to the field); the Params/Headers cells, the raw
//! body and the Auth fields follow in a later wiring step, each wrapping its input the
//! same way.

use std::collections::BTreeSet;
use std::sync::Arc;

use gpui::{
    Anchor, AnyElement, Context, ElementId, Entity, Focusable, Global, InteractiveElement,
    IntoElement, ParentElement, Render, Role, ScrollStrategy, SharedString,
    StatefulInteractiveElement, Styled, Subscription, UniformListScrollHandle, Window, anchored,
    deferred,
};
use pustman_actions::request::Cancel as CancelRequest;
use text_input::{TextInput, TextInputEvent, completion::Completion};
use ui::prelude::*;
use variables::ScopeStack;

/// The key context the popover answers the keyboard in: the arrows move the selection
/// and Tab inserts it. The field's own `"TextInput"` context nests inside this one — the
/// `command_palette::picker` shape.
pub const VARIABLE_COMPLETION_CONTEXT: &str = "VariableCompletion";

/// Marks that this app's keymap already carries [`VARIABLE_COMPLETION_CONTEXT`]'s
/// bindings. [`VariableAutocomplete::new`] is the only door this crate has to bind them
/// (the crate's `init` is outside this step's scope), and a second field must not stack
/// duplicates.
struct CompletionKeysBound;

impl Global for CompletionKeysBound {}

/// One match row's height in pixels at the default scale. Rows are uniform because the
/// virtualized list measures one row for all of them.
const ROW_HEIGHT_PX: f32 = 28.;

/// How many rows the popover shows before it starts scrolling.
const MAX_VISIBLE_ROWS: usize = 8;

/// The width of the popover surface in pixels at the default scale — room for a long
/// variable name; a longer one truncates, with the full name in the row's tooltip.
const POPOVER_WIDTH_PX: f32 = 320.;

/// The `{{variable}}` popover behind one field: the trigger at the field's caret
/// ([`text_input::completion`]), the names it matches across the scopes in effect, and
/// the selection the keyboard and the mouse move.
pub struct VariableAutocomplete {
    /// The field the popover completes — the one it listens to and inserts into.
    input: Entity<TextInput>,
    /// The scopes the names come from (REQ-VAR-04) — the editor's stack, shared with
    /// `variable_highlight` the same way.
    scopes: Arc<ScopeStack>,
    /// The trigger at the field's caret, while one is open.
    completion: Option<Completion>,
    /// The names the trigger matches, in popover order: narrowest scope first, then by
    /// name (see [`names_in_scope`]).
    matches: Vec<String>,
    /// The row the selection is on.
    selected: usize,
    /// The list's scroll state, so moving the selection scrolls its row into view.
    scroll_handle: UniformListScrollHandle,
    // Kept so the field's edits keep reaching us; dropping it would silently stop the
    // popover from following the caret.
    _input_subscription: Subscription,
}

impl VariableAutocomplete {
    /// The autocomplete behind `input`, closed. `Decision:` the popover's keys are bound
    /// here (the `history_panel` shape for a crate whose `init` a step may not edit):
    /// once per app, so a second field never stacks duplicate bindings.
    pub fn new(input: Entity<TextInput>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        if cx.try_global::<CompletionKeysBound>().is_none() {
            cx.set_global(CompletionKeysBound);
            cx.bind_keys([
                gpui::KeyBinding::new("down", SelectNext, Some(VARIABLE_COMPLETION_CONTEXT)),
                gpui::KeyBinding::new("up", SelectPrevious, Some(VARIABLE_COMPLETION_CONTEXT)),
                gpui::KeyBinding::new("tab", Activate, Some(VARIABLE_COMPLETION_CONTEXT)),
            ]);
        }
        let subscription = cx.subscribe_in(&input, window, Self::on_input_event);
        Self {
            input,
            scopes: Arc::new(ScopeStack::new()),
            completion: None,
            matches: Vec::new(),
            selected: 0,
            scroll_handle: UniformListScrollHandle::new(),
            _input_subscription: subscription,
        }
    }

    /// Store the scopes the names come from and re-run the trigger against them — the
    /// same `set_scopes` shape as `variable_highlight`.
    pub fn set_scopes(&mut self, scopes: Arc<ScopeStack>, cx: &mut Context<Self>) {
        self.scopes = scopes;
        self.refresh(cx);
    }

    /// The names the popover lists right now, in order — what the list draws and a test
    /// reads it by.
    pub fn matches(&self) -> &[String] {
        &self.matches
    }

    /// Whether the popover is showing: the caret sits at a trigger that matches at least
    /// one name (REQ-VAR-04 keeps it closed on an empty result).
    pub fn is_open(&self) -> bool {
        !self.matches.is_empty()
    }

    /// The field's edits drive the popover: every change re-runs the trigger at the
    /// caret, and Enter — which the focused field turns into
    /// [`TextInputEvent::Submitted`] — inserts the selection while the popover is open
    /// (the `command_palette::picker` contract).
    fn on_input_event(
        &mut self,
        _input: &Entity<TextInput>,
        event: &TextInputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextInputEvent::Changed => self.refresh(cx),
            TextInputEvent::Submitted => self.accept(window, cx),
        }
    }

    /// Recompute the trigger and the names it matches against the current scopes. An
    /// empty result keeps the popover closed.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.completion = self.input.read(cx).completion();
        self.matches = match &self.completion {
            Some(completion) => names_in_scope(&self.scopes)
                .into_iter()
                .filter(|name| matches_query(name, completion.query()))
                .collect(),
            None => Vec::new(),
        };
        self.selected = 0;
        cx.notify();
    }

    /// Move the selection one row, wrapping around the ends (the
    /// `command_palette::picker` contract, so the arrows never fall off the list).
    fn move_selection(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.matches.len();
        if count == 0 {
            return;
        }
        self.selected = if forward {
            (self.selected + 1) % count
        } else {
            (self.selected + count - 1) % count
        };
        self.scroll_handle
            .scroll_to_item(self.selected, ScrollStrategy::Top);
        cx.notify();
    }

    /// Insert the selection. Enter, Tab and a click on a row all land here — the one
    /// activation path, so the keyboard and the mouse can never disagree.
    fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.matches.get(self.selected).cloned() else {
            return;
        };
        self.insert_name(&name, window, cx);
    }

    /// Replace the trigger with `name}}` and close the popover.
    fn insert_name(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(completion) = self.completion.take() else {
            return;
        };
        self.close(cx);
        self.input.update(cx, |input, cx| {
            input.insert_completion(&completion, name, window, cx);
        });
    }

    /// Close the popover and clear the selection with it.
    fn close(&mut self, cx: &mut Context<Self>) {
        self.completion = None;
        self.matches.clear();
        self.selected = 0;
        cx.notify();
    }

    /// The arrows step the selection while the popover is open. With it closed these
    /// keystrokes are not this field's to consume (the `headers` suggestion contract)
    /// and keep bubbling.
    fn select_next(&mut self, _: &SelectNext, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_open() {
            cx.propagate();
            return;
        }
        self.move_selection(true, cx);
    }

    fn select_previous(
        &mut self,
        _: &SelectPrevious,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_open() {
            cx.propagate();
            return;
        }
        self.move_selection(false, cx);
    }

    /// Tab inserts the selection; Enter arrives as the field's
    /// [`TextInputEvent::Submitted`] instead (the `command_palette::picker` shape).
    fn activate(&mut self, _: &Activate, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_open() {
            cx.propagate();
            return;
        }
        self.accept(window, cx);
    }

    /// `request::Cancel` (Escape) closes the popover — the URL bar's "back out of what
    /// is open" contract (`UrlBar::cancel`'s custom-method field does the same). With no
    /// popover open the action keeps bubbling, so it still cancels the send in flight.
    fn cancel(&mut self, _: &CancelRequest, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_open() {
            cx.propagate();
            return;
        }
        self.close(cx);
    }

    /// The popover under the field: the anchored floating surface (the `auth_tab` menu
    /// shape) holding the virtualized name list. [`None`] while nothing matches.
    fn render_popover(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.is_open() {
            return None;
        }
        // Every element id in this popover derives from the field's focus handle — the
        // control's identity — so two fields can never collide in one frame (the
        // `command_palette::picker` shape).
        let base_id = ElementId::from(&self.input.read(cx).focus_handle(cx));
        // `VirtualList`'s row-key closure has no `App` to ask us with, so the stable
        // keys are snapshotted here (the picker's shape).
        let row_keys: Vec<ElementId> = self
            .matches
            .iter()
            .map(|name| row_element_id(name))
            .collect();
        let visible_rows = self.matches.len().clamp(1, MAX_VISIBLE_ROWS);
        let list_height = px_to_rems(ROW_HEIGHT_PX * visible_rows as f32);
        let list = VirtualList::new(
            ElementId::NamedChild(Arc::new(base_id.clone()), SharedString::new_static("rows")),
            row_keys.len(),
            move |index| {
                row_keys
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| ElementId::from("completion-row"))
            },
            cx.processor(|this, index: usize, _window, cx| this.render_row(index, cx)),
        )
        .track_scroll(self.scroll_handle.clone());
        let surface = v_flex()
            .id(ElementId::NamedChild(
                Arc::new(base_id),
                SharedString::new_static("completions"),
            ))
            .role(Role::ListBox)
            .aria_label(SharedString::new_static("Variable names"))
            .elevation_2(cx)
            .rounded_sm()
            .py(DynamicSpacing::Base02.rems(cx))
            .w(px_to_rems(POPOVER_WIDTH_PX))
            .max_w_full()
            .child(v_flex().w_full().h(list_height).child(list))
            .child(
                Label::new(SharedString::new_static(
                    "Up/Down choose - Enter accepts - Escape dismisses",
                ))
                .size(LabelSize::XSmall)
                .color(Color::Muted),
            );
        Some(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .child(
                    deferred(
                        anchored()
                            .anchor(Anchor::TopLeft)
                            .snap_to_window_with_margin(DynamicSpacing::Base04.px(cx))
                            .child(surface),
                    )
                    .with_priority(1),
                )
                .into_any_element(),
        )
    }

    /// One match row: the variable's name, with the selection's highlight and the click
    /// that inserts it. The clickable container is a child id of the row's stable key
    /// (the `command_palette::picker`'s row shape), so a click inserts *this* name even
    /// if the list re-orders underneath the pointer.
    fn render_row(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(name) = self.matches.get(index).cloned() else {
            // The list is told the row count of the same snapshot it draws keys from,
            // so an out-of-range index cannot occur — and must not panic if it ever does.
            return div().into_any_element();
        };
        let selected = index == self.selected;
        // The click inserts *this* name: the closure captures the name, never the row
        // index the list may have shifted by the time it runs.
        let clicked_name = name.clone();
        let row_id = ElementId::NamedChild(
            Arc::new(row_element_id(&name)),
            SharedString::new_static("row"),
        );
        h_flex()
            .id(row_id)
            .role(Role::ListBoxOption)
            .aria_selected(selected)
            .aria_label(name.clone())
            .h(px_to_rems(ROW_HEIGHT_PX))
            .min_w_0()
            .items_center()
            .gap(DynamicSpacing::Base02.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .rounded_sm()
            .cursor_pointer()
            .hover(|row| row.bg(cx.theme().colors().ghost_element_hover))
            .when(selected, |row| row.bg(cx.theme().colors().element_selected))
            .tooltip(Tooltip::text(name.clone()).builder())
            .on_click(cx.listener(move |this, _event, window, cx| {
                this.insert_name(&clicked_name, window, cx);
            }))
            .child(Label::new(name).size(LabelSize::Small).truncate())
            .into_any_element()
    }
}

impl Render for VariableAutocomplete {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(VARIABLE_COMPLETION_CONTEXT)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::activate))
            .on_action(cx.listener(Self::cancel))
            .relative()
            .child(self.input.clone())
            .children(self.render_popover(cx))
    }
}

/// One row's stable identity: the name it shows — never the row's position, which
/// shifts as the query narrows (the `command_palette::picker`'s match-key shape).
fn row_element_id(name: &str) -> ElementId {
    ElementId::from(format!("completion-{name}"))
}

/// Every enabled variable name the scopes hold, in popover order: narrowest scope
/// first, then by name within a scope, and each name listed once — a name a narrower
/// scope shadows lists at that scope's position only (REQ-VAR-04). Only names are read
/// here: a value never leaves the [`ScopeStack`]. Variables a scope has switched off
/// resolve to nothing (`ScopeStack::find` skips them), so their names are not offered.
fn names_in_scope(scopes: &ScopeStack) -> Vec<String> {
    let mut names = Vec::new();
    let mut listed = BTreeSet::new();
    for scope in scopes.scopes() {
        let mut in_scope: Vec<&str> = scope
            .variables
            .iter()
            .filter(|variable| variable.enabled)
            .map(|variable| variable.key.as_str())
            .collect();
        in_scope.sort_unstable();
        for name in in_scope {
            if listed.insert(name) {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// Whether `name` belongs in the popover for `typed`: the typed partial as a prefix or
/// a substring, case-insensitively (REQ-VAR-04).
fn matches_query(name: &str, typed: &str) -> bool {
    name.to_lowercase().contains(&typed.to_lowercase())
}

#[cfg(test)]
#[path = "variable_autocomplete_tests.rs"]
mod tests;
