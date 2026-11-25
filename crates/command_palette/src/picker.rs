//! The reusable modal picker: a [`TextInput`] query over a keyboard-driven, virtualized
//! list of matches.
//!
//! A picker is one control: the [`PickerDelegate`] owns the items and what confirming
//! one means, the [`Picker`] owns the query field, the selection mechanics and the
//! dismissal. Zed's `picker` crate is GPL — this is our own shape of the same idea, and
//! it satisfies gpui's `ManagedView` (`Render + Focusable + EventEmitter<DismissEvent>`)
//! so S22d's modal layer can host it when that lands.
//!
//! `Decision:` the picker lives in `command_palette` rather than `ui` because it owns an
//! `Entity<TextInput>` and `ui` must not depend on `text_input`; S34a, S41b and S46
//! depend on this crate to reuse it.
//!
//! `Decision:` the selection wraps around the ends of the list (like `ui::ContextMenu`),
//! so the arrows never fall off it.

use std::sync::Arc;

use gpui::{
    AnyElement, App, ClickEvent, Context, DismissEvent, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, InteractiveElement, ParentElement, Render, Role, ScrollStrategy,
    SharedString, StatefulInteractiveElement, Styled, Subscription, UniformListScrollHandle,
    Window, actions,
};
use text_input::{TextInput, TextInputEvent};
use ui::prelude::*;

/// The key context the picker answers the keyboard in; [`crate::init`] binds the arrows,
/// Enter and Escape. The query field's own `"TextInput"` context nests inside it.
pub const PICKER_CONTEXT: &str = "Picker";

/// One match row's height in pixels at the default scale. Rows are uniform because
/// [`VirtualList`] measures one row for all of them.
const ROW_HEIGHT_PX: f32 = 28.;

/// The width of the picker surface in pixels at the default scale.
const PICKER_WIDTH_PX: f32 = 512.;

/// How many rows the match list shows before it starts scrolling.
const MAX_VISIBLE_ROWS: usize = 10;

actions!(
    command_palette,
    [
        /// Moves the selection to the next match.
        SelectNext,
        /// Moves the selection to the previous match.
        SelectPrevious,
        /// Confirms the selected match.
        Confirm,
        /// Dismisses the picker without confirming anything.
        Cancel,
    ]
);

/// What a [`Picker`] asks of the screen that hosts it: the items, how they are drawn,
/// and what confirming or dismissing one does.
///
/// `Decision:` matching is synchronous in this step ([`Self::update_matches`] runs on
/// every keystroke, on the UI thread) — the candidate lists behind v1 pickers are small
/// enough, and an async matcher is a later improvement.
pub trait PickerDelegate: Sized + 'static {
    /// How many matches the current query has.
    fn match_count(&self) -> usize;

    /// Which match the selection is on, an index below [`Self::match_count`].
    fn selected_index(&self) -> usize;

    /// Move the selection. Implementations record it and call `cx.notify()`.
    fn set_selected_index(&mut self, index: usize, cx: &mut Context<Picker<Self>>);

    /// Recompute the matches for a new query. The picker resets the selection to the
    /// first match afterwards.
    fn update_matches(&mut self, query: &str, cx: &mut Context<Picker<Self>>);

    /// The user confirmed a match — Enter or a click. The delegate decides what that
    /// means (open the item, apply the value) and whether the picker dismisses, by
    /// emitting [`DismissEvent`] on `cx`.
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>);

    /// The user backed out — Escape. The picker emits [`DismissEvent`] itself after
    /// this, so a delegate only cleans up its own state here.
    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>);

    /// Draw one match row. Rows must be one uniform height
    /// ([`VirtualList`] measures the first row for all of them) and truncate their text.
    fn render_match(
        &self,
        index: usize,
        selected: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;

    /// The stable element id of the item behind match `index` — its identity, never its
    /// row position, which shifts as the query re-ranks.
    fn match_key(&self, index: usize) -> ElementId;

    /// The hint shown in the query field while it is empty; it is also the field's
    /// accessible label.
    fn placeholder_text(&self) -> SharedString;

    /// A control drawn at the right of the query field — the environment menu's "＋"
    /// (S48u6). Most pickers have none.
    fn render_query_accessory(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<AnyElement> {
        None
    }

    /// A footer drawn under the matches — the environment menu's "Edit environments…"
    /// (S48u6). Most pickers have none.
    fn render_footer(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<AnyElement> {
        None
    }
}

/// A modal picker: the query [`TextInput`], the delegate's matches as a virtualized
/// list, and the keyboard contract — Down and Up move the selection (wrapping), Enter
/// confirms, Escape dismisses. Clicking a row selects and confirms it.
///
/// The list is drawn with [`VirtualList`] over a scroll handle, so the selection is
/// scrolled into view as it moves. Build it with a window — the query field needs one:
/// `cx.new(|cx| Picker::new(delegate, window, cx))`.
pub struct Picker<D: PickerDelegate> {
    delegate: D,
    query: Entity<TextInput>,
    scroll_handle: UniformListScrollHandle,
    // Kept so the query's events keep reaching us; dropping it would silently stop
    // filtering.
    _query_subscription: Subscription,
}

impl<D: PickerDelegate> Picker<D> {
    /// A picker over `delegate`'s matches, with an empty query.
    pub fn new(delegate: D, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = delegate.placeholder_text();
        let query = cx.new(|cx| TextInput::new(window, cx).with_placeholder(placeholder));
        let query_subscription = cx.subscribe_in(&query, window, Self::on_query_event);
        Self {
            delegate,
            query,
            scroll_handle: UniformListScrollHandle::new(),
            _query_subscription: query_subscription,
        }
    }

    /// The delegate behind the matches — the host reads its state through this.
    pub fn delegate(&self) -> &D {
        &self.delegate
    }

    /// The query field drives everything else: an edit re-ranks and re-selects, Enter
    /// (which the focused field turns into `Submitted`) confirms.
    ///
    /// This runs inside the picker's own update (`this` is the picker), never through a
    /// nested `Picker::update`.
    fn on_query_event(
        &mut self,
        query: &Entity<TextInput>,
        event: &TextInputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextInputEvent::Changed => {
                let text = query.read(cx).text();
                self.delegate.update_matches(&text, cx);
                // A new query starts at its top match — but an empty result list has no
                // selection to reset to (the delegate indexes below its match_count).
                if self.delegate.match_count() > 0 {
                    self.select_match(0, cx);
                }
                cx.notify();
            }
            TextInputEvent::Submitted => self.confirm_selection(window, cx),
        }
    }

    /// Select `index` and keep the visible row in step with it: the one selection
    /// path, shared by the arrow keys and [`Self::on_query_event`]'s reset to the top
    /// match.
    fn select_match(&mut self, index: usize, cx: &mut Context<Self>) {
        self.delegate.set_selected_index(index, cx);
        self.scroll_handle
            .scroll_to_item(index, ScrollStrategy::Top);
        cx.notify();
    }

    /// The one place that decides whether there is a selection to confirm. An empty
    /// match list has none, so Enter and [`Confirm`] then do nothing at all — no
    /// confirm, no dismissal — instead of confirming a row that does not exist.
    fn confirm_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.delegate.match_count() == 0 {
            return;
        }
        self.delegate.confirm(window, cx);
    }

    /// Move the selection one row, wrapping around the ends. An empty match list moves
    /// nothing.
    fn move_selection(&mut self, forward: bool, cx: &mut Context<Self>) {
        let match_count = self.delegate.match_count();
        if match_count == 0 {
            return;
        }
        let selected = self.delegate.selected_index().min(match_count - 1);
        let next = if forward {
            (selected + 1) % match_count
        } else {
            (selected + match_count - 1) % match_count
        };
        self.select_match(next, cx);
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

    fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_selection(window, cx);
    }

    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        self.delegate.dismissed(window, cx);
        cx.emit(DismissEvent);
    }

    /// The one path a match is confirmed by — a row click after selecting it, and Enter
    /// through [`Self::on_query_event`] and the [`Confirm`] action — so the mouse and
    /// the keyboard can never disagree about what a match does.
    fn click_match(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.delegate.set_selected_index(index, cx);
        cx.notify();
        self.confirm_selection(window, cx);
    }

    /// One row: the delegate's content in a selectable, clickable container whose id and
    /// accessibility tree node come from the item's stable key.
    fn render_match_row(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = index == self.delegate.selected_index();
        let content = self.delegate.render_match(index, selected, window, cx);
        let key = self.delegate.match_key(index);
        let row_id = ElementId::NamedChild(Arc::new(key), SharedString::new_static("row"));
        h_flex()
            .id(row_id)
            .role(Role::ListBoxOption)
            .aria_selected(selected)
            .w_full()
            .min_w_0()
            .min_h(px_to_rems(ROW_HEIGHT_PX))
            .mx(DynamicSpacing::Base02.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .rounded_sm()
            .cursor_pointer()
            .when(selected, |row| row.bg(cx.theme().colors().element_selected))
            .hover(|row| row.bg(cx.theme().colors().ghost_element_hover))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.click_match(index, window, cx);
            }))
            .child(content)
            .into_any_element()
    }
}

impl<D: PickerDelegate> Focusable for Picker<D> {
    /// The picker is one tab stop: it delegates focus to its query field, so typing
    /// works the moment it opens.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.query.focus_handle(cx)
    }
}

impl<D: PickerDelegate> EventEmitter<DismissEvent> for Picker<D> {}

impl<D: PickerDelegate> Render for Picker<D> {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query_accessory = self.delegate.render_query_accessory(window, cx);
        let footer = self.delegate.render_footer(window, cx);
        let match_count = self.delegate.match_count();
        // The picker's own element ids derive from its focus handle — its identity — so
        // two pickers can never collide in one frame.
        let picker_id = ElementId::from(&self.query.focus_handle(cx));
        // `VirtualList`'s row-key closure has no `App` to ask the delegate with, so the
        // stable item keys are snapshotted here, where the delegate is still readable.
        let row_keys: Vec<ElementId> = (0..match_count)
            .map(|index| self.delegate.match_key(index))
            .collect();
        let visible_rows = match_count.clamp(1, MAX_VISIBLE_ROWS);
        let list_height = px_to_rems(ROW_HEIGHT_PX * visible_rows as f32);
        let list = VirtualList::new(
            ElementId::NamedChild(Arc::new(picker_id), SharedString::new_static("matches")),
            match_count,
            move |index| {
                row_keys
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| ElementId::from("picker-row"))
            },
            cx.processor(move |this, index: usize, window, cx| {
                this.render_match_row(index, window, cx)
            }),
        )
        .track_scroll(self.scroll_handle.clone());

        // `Decision:` the picker draws its own elevated surface instead of `ui::Modal`
        // chrome: `Modal` needs a title the delegate contract does not have, and S22d's
        // modal layer will host this view — the surface is all the chrome it owns.
        v_flex()
            .key_context(PICKER_CONTEXT)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::cancel))
            .elevation_3(cx)
            .font_ui(cx)
            .w(px_to_rems(PICKER_WIDTH_PX))
            .max_w_full()
            .p(DynamicSpacing::Base08.rems(cx))
            .gap(DynamicSpacing::Base06.rems(cx))
            .child(
                h_flex()
                    .w_full()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .border_1()
                            .border_color(cx.theme().colors().border)
                            .rounded_sm()
                            .px(DynamicSpacing::Base04.rems(cx))
                            .child(self.query.clone()),
                    )
                    .children(query_accessory),
            )
            .map(|picker| {
                if match_count == 0 {
                    picker.child(
                        Label::new("No matches")
                            .color(Color::Muted)
                            .size(LabelSize::Small),
                    )
                } else {
                    picker.child(
                        v_flex()
                            .id(ElementId::NamedChild(
                                Arc::new(ElementId::from(&self.query.focus_handle(cx))),
                                SharedString::new_static("match-list"),
                            ))
                            .role(Role::ListBox)
                            .w_full()
                            .h(list_height)
                            .child(list),
                    )
                }
            })
            .children(footer.map(|footer| {
                div()
                    .w_full()
                    .pt(DynamicSpacing::Base04.rems(cx))
                    .border_t_1()
                    .border_color(cx.theme().colors().border_variant)
                    .child(footer)
            }))
    }
}

#[cfg(test)]
#[path = "picker_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "picker_empty_tests.rs"]
mod empty_tests;
