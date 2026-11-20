//! The empty-match-list contract: with no matches there is no selection, so nothing can
//! be confirmed — Enter and the [`Confirm`] action must do exactly nothing (no confirm,
//! no dismissal), and a re-filter must never hand the delegate a selection index outside
//! its match list.
//!
//! `picker_tests.rs`'s `FilterDelegate` is private to that module and existing test
//! files are read-only, so this file carries its own minimal probe delegate. It records
//! every out-of-range selection instead of panicking the way a natural delegate
//! (`self.matches[self.selected]`) would, so one assertion can report what the picker
//! asked for.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, DismissEvent, ElementId, Entity, Focusable, IntoElement,
    SharedString, Subscription, TestAppContext, VisualTestContext, Window,
};
use ui::Label;
use ui::test_support::{draw_window, init_test, press_key};

use super::{Confirm, Picker, PickerDelegate};

/// The candidates every test starts from: four the query `ab` can spell and one it
/// cannot — and none of them spells `zzz`.
const CANDIDATES: [&str; 5] = ["abacus", "alpha_bravo", "zab", "azb", "notes"];

/// A minimal delegate over fixed candidates: it filters with the crate's own
/// [`crate::fuzzy`] scorer and records what the picker asked of it.
struct ProbeDelegate {
    candidates: Vec<String>,
    matches: Vec<usize>,
    selected: usize,
    confirmed: Option<usize>,
    dismissed: bool,
    out_of_range_selections: Vec<usize>,
}

impl ProbeDelegate {
    fn new(candidates: &[&str]) -> Self {
        Self {
            candidates: candidates.iter().map(|item| (*item).to_string()).collect(),
            matches: (0..candidates.len()).collect(),
            selected: 0,
            confirmed: None,
            dismissed: false,
            out_of_range_selections: Vec::new(),
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

impl PickerDelegate for ProbeDelegate {
    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected
    }

    fn set_selected_index(&mut self, index: usize, cx: &mut Context<Picker<Self>>) {
        if index >= self.matches.len() {
            self.out_of_range_selections.push(index);
        }
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
fn open_picker(cx: &mut TestAppContext) -> (Entity<Picker<ProbeDelegate>>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        crate::init(cx);
    });
    let (picker, cx) =
        cx.add_window_view(|window, cx| Picker::new(ProbeDelegate::new(&CANDIDATES), window, cx));
    draw_window(cx);
    let focus_handle = cx.read(|cx| picker.read_with(cx, |picker, cx| picker.focus_handle(cx)));
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    (picker, cx)
}

/// How many matches the delegate currently shows.
fn match_count_of(picker: &Entity<Picker<ProbeDelegate>>, cx: &mut VisualTestContext) -> usize {
    cx.read(|cx| picker.read_with(cx, |picker, _| picker.delegate().match_count()))
}

/// Whether the delegate's own dismissal cleanup ran.
fn dismissed_of(picker: &Entity<Picker<ProbeDelegate>>, cx: &mut VisualTestContext) -> bool {
    cx.read(|cx| picker.read_with(cx, |picker, _| picker.delegate().dismissed))
}

/// What the delegate confirmed, if anything.
fn confirmation_of(
    picker: &Entity<Picker<ProbeDelegate>>,
    cx: &mut VisualTestContext,
) -> Option<usize> {
    cx.read(|cx| picker.read_with(cx, |picker, _| picker.delegate().confirmed))
}

/// Every selection index the picker asked for that was outside the delegate's match
/// list.
fn out_of_range_selections_of(
    picker: &Entity<Picker<ProbeDelegate>>,
    cx: &mut VisualTestContext,
) -> Vec<usize> {
    cx.read(|cx| {
        picker.read_with(cx, |picker, _| {
            picker.delegate().out_of_range_selections.clone()
        })
    })
}

/// A counter of the [`DismissEvent`]s the picker has emitted — the observer must
/// outlive the assertions, and `DismissEvent` is not `Clone`, so the stream cannot be
/// drained.
fn count_dismissals(
    picker: &Entity<Picker<ProbeDelegate>>,
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
fn enter_on_an_empty_match_list_confirms_nothing(cx: &mut TestAppContext) {
    let (picker, cx) = open_picker(cx);
    let (dismissals, _subscription) = count_dismissals(&picker, cx);
    cx.run_until_parked();

    cx.simulate_input("zzz");
    draw_window(cx);
    assert_eq!(
        match_count_of(&picker, cx),
        0,
        "the query \"zzz\" matches no candidate"
    );

    press_key(cx, "enter");
    cx.dispatch_action(Confirm);
    cx.run_until_parked();

    assert_eq!(
        confirmation_of(&picker, cx),
        None,
        "neither Enter through the query field nor the Confirm action confirms a row \
         that does not exist"
    );
    assert_eq!(
        dismissals.get(),
        0,
        "an unconfirmed picker is not dismissed"
    );
    assert!(
        !dismissed_of(&picker, cx),
        "the delegate's dismissal cleanup does not run either"
    );
}

#[gpui::test]
fn typing_with_no_matches_never_selects_out_of_range(cx: &mut TestAppContext) {
    let (picker, cx) = open_picker(cx);

    press_key(cx, "down");
    press_key(cx, "down");
    cx.simulate_input("zzz");
    draw_window(cx);

    assert_eq!(
        match_count_of(&picker, cx),
        0,
        "the query \"zzz\" matches no candidate"
    );
    assert!(
        out_of_range_selections_of(&picker, cx).is_empty(),
        "re-filtering to no matches never selects an index outside the match list: {:?}",
        out_of_range_selections_of(&picker, cx)
    );
}
