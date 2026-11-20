//! The picker is driven the way a user drives it: a real window with focus, the query
//! through the focused [`TextInput`], and the keys [`crate::init`] binds. Every test
//! asserts an outcome the delegate can see — filtering, ranking, selection, confirm,
//! dismissal — so a picker that renders but never filters or emits fails them.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, DismissEvent, ElementId, Entity, Focusable, IntoElement, Modifiers,
    SharedString, Subscription, TestAppContext, VisualTestContext, Window,
};
use ui::Label;
use ui::test_support::{draw_window, init_test, press_key};

use super::{Picker, PickerDelegate};

/// The candidates every test starts from: four the query `ab` can spell, in the scorer's
/// tier order, and one it cannot.
const CANDIDATES: [&str; 5] = ["abacus", "alpha_bravo", "zab", "azb", "notes"];

/// A small delegate over fixed candidates: it filters and ranks them with the crate's
/// own [`crate::fuzzy`] scorer and records what the picker asked of it.
struct FilterDelegate {
    candidates: Vec<String>,
    matches: Vec<usize>,
    selected: usize,
    confirmed: Option<usize>,
    dismissed: bool,
}

impl FilterDelegate {
    fn new(candidates: &[&str]) -> Self {
        Self {
            candidates: candidates.iter().map(|item| (*item).to_string()).collect(),
            matches: (0..candidates.len()).collect(),
            selected: 0,
            confirmed: None,
            dismissed: false,
        }
    }

    /// The candidate behind match `index`, if there is one.
    fn candidate_at(&self, index: usize) -> Option<&str> {
        self.matches
            .get(index)
            .and_then(|candidate_index| self.candidates.get(*candidate_index))
            .map(String::as_str)
    }
}

impl PickerDelegate for FilterDelegate {
    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected
    }

    fn set_selected_index(&mut self, index: usize, cx: &mut Context<Picker<Self>>) {
        self.selected = index;
        cx.notify();
    }

    fn update_matches(&mut self, query: &str, cx: &mut Context<Picker<Self>>) {
        self.matches = crate::fuzzy::rank(query, self.candidates.iter().map(String::as_str))
            .into_iter()
            .map(|string_match| string_match.candidate_index)
            .collect();
        cx.notify();
    }

    fn confirm(&mut self, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.confirmed = Some(self.selected);
        cx.emit(DismissEvent);
    }

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {
        self.dismissed = true;
    }

    fn render_match(
        &self,
        index: usize,
        _selected: bool,
        _window: &mut Window,
        _cx: &mut App,
    ) -> AnyElement {
        Label::new(self.candidate_at(index).unwrap_or_default().to_string())
            .truncate()
            .into_any_element()
    }

    fn match_key(&self, index: usize) -> ElementId {
        let candidate_index = self.matches.get(index).copied().unwrap_or(usize::MAX);
        ElementId::from(format!("match-{candidate_index}"))
    }

    fn placeholder_text(&self) -> SharedString {
        SharedString::new_static("Filter matches")
    }
}

/// A window with one picker over [`CANDIDATES`], drawn and focused on its query field,
/// ready for keystrokes.
fn open_picker(
    cx: &mut TestAppContext,
) -> (Entity<Picker<FilterDelegate>>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        crate::init(cx);
    });
    let (picker, cx) =
        cx.add_window_view(|window, cx| Picker::new(FilterDelegate::new(&CANDIDATES), window, cx));
    draw_window(cx);
    let focus_handle = cx.read(|cx| picker.read_with(cx, |picker, cx| picker.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    (picker, cx)
}

/// The match indices the delegate currently shows, in rank order.
fn matches_of(picker: &Entity<Picker<FilterDelegate>>, cx: &mut VisualTestContext) -> Vec<usize> {
    cx.read(|cx| picker.read_with(cx, |picker, _| picker.delegate().matches.clone()))
}

/// Which match the selection is on.
fn selection_of(picker: &Entity<Picker<FilterDelegate>>, cx: &mut VisualTestContext) -> usize {
    cx.read(|cx| picker.read_with(cx, |picker, _| picker.delegate().selected_index()))
}

/// What the delegate confirmed, if anything.
fn confirmation_of(
    picker: &Entity<Picker<FilterDelegate>>,
    cx: &mut VisualTestContext,
) -> Option<usize> {
    cx.read(|cx| picker.read_with(cx, |picker, _| picker.delegate().confirmed))
}

/// A counter of the [`DismissEvent`]s the picker has emitted. The subscription behind
/// it is the observer — `gpui`'s `DismissEvent` is not `Clone`, so the event stream
/// cannot be drained — and it must outlive the assertions.
fn count_dismissals(
    picker: &Entity<Picker<FilterDelegate>>,
    cx: &mut VisualTestContext,
) -> (Rc<Cell<usize>>, Subscription) {
    let dismissals = Rc::new(Cell::new(0));
    let subscription = cx.update({
        let dismissals = dismissals.clone();
        move |_, cx| {
            cx.subscribe(picker, move |_, _: &DismissEvent, _| {
                dismissals.set(dismissals.get() + 1)
            })
        }
    });
    (dismissals, subscription)
}

#[gpui::test]
fn escape_dismisses(cx: &mut TestAppContext) {
    let (picker, cx) = open_picker(cx);
    let (dismissals, _subscription) = count_dismissals(&picker, cx);
    cx.run_until_parked();

    press_key(cx, "escape");

    assert!(
        cx.read(|cx| picker.read_with(cx, |picker, _| picker.delegate().dismissed)),
        "escape hands the dismissal to the delegate"
    );
    assert_eq!(
        dismissals.get(),
        1,
        "escape emits DismissEvent exactly once"
    );
}

#[gpui::test]
fn typing_filters_and_ranks_the_list(cx: &mut TestAppContext) {
    let (picker, cx) = open_picker(cx);

    cx.simulate_input("ab");
    draw_window(cx);

    assert_eq!(
        matches_of(&picker, cx),
        vec![0, 1, 2, 3],
        "typing keeps only the subsequence matches — prefix, word start, contiguous, \
         scattered — and drops \"notes\""
    );
    assert!(
        cx.debug_bounds("match-0").is_some(),
        "the top-ranked match is rendered"
    );
    assert!(
        cx.debug_bounds("match-4").is_none(),
        "a filtered-out candidate is not rendered"
    );
}

#[gpui::test]
fn down_and_up_move_the_selection_with_wrap(cx: &mut TestAppContext) {
    let (picker, cx) = open_picker(cx);

    press_key(cx, "down");
    assert_eq!(
        selection_of(&picker, cx),
        1,
        "down moves the selection to the second match"
    );

    press_key(cx, "up");
    press_key(cx, "up");
    assert_eq!(
        selection_of(&picker, cx),
        4,
        "up from the first match wraps to the last"
    );

    press_key(cx, "down");
    assert_eq!(
        selection_of(&picker, cx),
        0,
        "down from the last match wraps to the first"
    );
}

#[gpui::test]
fn enter_confirms_the_selected_match(cx: &mut TestAppContext) {
    let (picker, cx) = open_picker(cx);
    let (dismissals, _subscription) = count_dismissals(&picker, cx);
    cx.run_until_parked();

    press_key(cx, "down");
    press_key(cx, "enter");

    assert_eq!(
        confirmation_of(&picker, cx),
        Some(1),
        "enter confirms the selected match"
    );
    assert_eq!(
        dismissals.get(),
        1,
        "the delegate chose to dismiss on confirm, and the picker emitted DismissEvent"
    );
}

#[gpui::test]
fn mouse_click_selects_and_confirms(cx: &mut TestAppContext) {
    let (picker, cx) = open_picker(cx);

    let bounds = cx
        .debug_bounds("match-2")
        .expect("the third match is painted");
    cx.simulate_click(bounds.center(), Modifiers::default());

    assert_eq!(
        selection_of(&picker, cx),
        2,
        "a click selects the row it lands on"
    );
    assert_eq!(
        confirmation_of(&picker, cx),
        Some(2),
        "and confirms it, the same way Enter confirms the selection"
    );
}
