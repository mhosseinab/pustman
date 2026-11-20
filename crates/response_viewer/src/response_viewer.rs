//! `response_viewer` — what a send comes back with, on screen: the status line
//! (REQ-RESP-01), the body (REQ-RESP-02), the headers table (REQ-RESP-05) and, when a
//! send failed, the error panel (REQ-RESP-08).
//!
//! [`ResponseView`] is the entity a host embeds (S29e puts it under the request editor).
//! It owns the status line, the Body/Headers tabs and the two panes below them, and takes
//! the send result through [`ResponseView::set_response`] — or its failure through
//! [`ResponseView::set_error`]. The input type is [`http_engine::Response`] — there is no
//! parallel model here.

mod body_view;
mod error_panel;
mod headers_view;
mod pretty;
mod status_line;

pub use body_view::BodyView;
pub use headers_view::{HeaderRow, HeadersView};
pub use status_line::StatusLine;

use std::sync::Arc;

use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, IntoElement, ParentElement, Render,
    Subscription, Window,
};
use http_engine::{Response, TransportError};
use ui::prelude::*;

use crate::error_panel::ErrorPanel;
use crate::status_line::body_size;

/// The key context the response viewer answers in. The default keymaps bind
/// `response::Find` here (`assets/keymaps/`, REQ-RESP-03); its handler lands with find.
const RESPONSE_VIEW_CONTEXT: &str = "ResponseViewer";

/// The debug key the body pane renders under — the test hook that lets
/// `VisualTestContext::debug_bounds` prove the body is (not) on screen (the `toast.rs`
/// pattern). The error panel replaces it, so the two never render together.
const BODY_PANE_SELECTOR: &str = "response-body-pane";

/// The tabs across the top of the response, in order. Later steps append here (Cookies,
/// Timeline); the bar is built from this list, so a new tab is one line and nothing else.
const TAB_TITLES: [&str; 2] = ["Body", "Headers"];
/// Where the body renders.
const BODY_TAB: usize = 0;
/// Where the headers table renders.
const HEADERS_TAB: usize = 1;

/// The response region: status line on top, tabs, and the selected pane below.
pub struct ResponseView {
    focus_handle: FocusHandle,
    /// The status line for the response on screen, precomputed when it arrived.
    status_line: Option<StatusLine>,
    tab_bar: Entity<TabBar>,
    selected_tab: usize,
    body: Entity<BodyView>,
    headers: Entity<HeadersView>,
    /// The failed send on screen instead of its body (REQ-RESP-08).
    error: Option<TransportError>,
    _tab_bar_subscription: Subscription,
}

impl ResponseView {
    /// An empty viewer: the "no response yet" empty state until [`Self::set_response`].
    pub fn new(cx: &mut Context<Self>) -> Self {
        let tab_bar = cx.new(|cx| TabBar::new(cx).with_tabs(tabs()));
        let _tab_bar_subscription = cx.subscribe(&tab_bar, Self::on_tab_bar_event);
        Self {
            focus_handle: cx.focus_handle(),
            status_line: None,
            tab_bar,
            selected_tab: BODY_TAB,
            body: cx.new(BodyView::new),
            headers: cx.new(HeadersView::new),
            error: None,
            _tab_bar_subscription,
        }
    }

    /// Show this response. It is dismantled once here — the body goes to [`BodyView`]
    /// without a copy, the headers to [`HeadersView`], and the status line precomputes
    /// its texts — so `render` only lays out what arrived.
    pub fn set_response(&mut self, response: Response, cx: &mut Context<Self>) {
        let response = Arc::new(response);
        self.error = None;
        self.status_line = Some(StatusLine::new(
            ElementId::from(&self.focus_handle),
            response.status,
            response.reason.clone(),
            response.timings.total,
            body_size(&response.body),
        ));
        self.headers
            .update(cx, |headers, cx| headers.set_headers(&response.headers, cx));
        self.body
            .update(cx, |body, cx| body.set_body(Arc::clone(&response), cx));
        cx.notify();
    }

    /// Show this failed send instead of a response body (REQ-RESP-08). The panel takes
    /// the body region only: the status line and tabs keep whatever they show until the
    /// next [`Self::set_response`] replaces both.
    pub fn set_error(&mut self, error: TransportError, cx: &mut Context<Self>) {
        self.error = Some(error);
        cx.notify();
    }

    /// What the region under the tabs shows: the error panel after a failed send
    /// (REQ-RESP-08), otherwise the selected tab's pane.
    fn pane(&self) -> AnyElement {
        match &self.error {
            Some(error) => {
                ErrorPanel::new(ElementId::from(&self.focus_handle), error).into_any_element()
            }
            None => match self.selected_tab {
                HEADERS_TAB => self.headers.clone().into_any_element(),
                _ => div()
                    .size_full()
                    .debug_selector(|| BODY_PANE_SELECTOR.to_owned())
                    .child(self.body.clone())
                    .into_any_element(),
            },
        }
    }

    /// The tab bar's events: moving the selection with the arrows or activating a tab
    /// with the mouse shows that tab's pane.
    fn on_tab_bar_event(
        &mut self,
        tab_bar: Entity<TabBar>,
        event: &TabBarEvent,
        cx: &mut Context<Self>,
    ) {
        let index = match event {
            TabBarEvent::Selected { index } | TabBarEvent::Activated { index } => *index,
        };
        // The bar's current selection is the truth; an event that no longer describes it
        // is a stale echo and is dropped (`workspace::pane_tabs`).
        if index != tab_bar.read(cx).selected() {
            return;
        }
        self.selected_tab = index;
        cx.notify();
    }
}

impl Render for ResponseView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id(ElementId::from(&self.focus_handle))
            .size_full()
            .min_h_0()
            .key_context(RESPONSE_VIEW_CONTEXT)
            .track_focus(&self.focus_handle)
            .map(|this| match &self.status_line {
                Some(status_line) => this
                    .child(status_line.clone())
                    .child(self.tab_bar.clone())
                    .child(v_flex().flex_1().min_h_0().child(self.pane())),
                // A failed send that never produced a response still gets its panel:
                // with no response the body region is the whole area (REQ-RESP-08).
                None => this.child(match &self.error {
                    Some(error) => ErrorPanel::new(ElementId::from(&self.focus_handle), error)
                        .into_any_element(),
                    None => empty_state(cx).into_any_element(),
                }),
            })
    }
}

impl Focusable for ResponseView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// The response viewer's init-order hook (`pustman::init` runs it after its
/// dependencies). S29a registers nothing globally: the views are created by their hosts,
/// and later steps register their actions here.
pub fn init(_cx: &mut App) {}

/// The tabs, built from [`TAB_TITLES`] so later steps append there and nowhere else.
fn tabs() -> Vec<Tab> {
    TAB_TITLES
        .iter()
        .enumerate()
        .map(|(index, title)| Tab::new(("response-tab", index), *title))
        .collect()
}

/// A child element id under `base`: a stable, frame-unique identity for an element that
/// carries a tooltip or otherwise stays stateful.
pub(crate) fn child_id(base: &ElementId, name: impl Into<SharedString>) -> ElementId {
    ElementId::from((base.clone(), name))
}

/// The state before the first response: what this region will show, and how to get one.
fn empty_state(cx: &App) -> impl IntoElement {
    v_flex()
        .size_full()
        .justify_center()
        .items_center()
        .gap(DynamicSpacing::Base04.rems(cx))
        .child(Headline::new("No response yet").size(HeadlineSize::Small))
        .child(
            Label::new("Send a request to see its status, headers and body here.")
                .color(Color::Muted),
        )
}
