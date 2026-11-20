//! `response_viewer` — what a send comes back with, on screen: the status line
//! (REQ-RESP-01), the body (REQ-RESP-02), the headers table (REQ-RESP-05), the cookies
//! table (REQ-RESP-02) and, when a
//! send failed, the error panel (REQ-RESP-08).
//!
//! [`ResponseView`] is the entity a host embeds (S29e puts it under the request editor).
//! It owns the status line, the Body/Headers tabs and the two panes below them, and takes
//! the send result through [`ResponseView::set_response`] — or its failure through
//! [`ResponseView::set_error`]. The input type is [`http_engine::Response`] — there is no
//! parallel model here.

mod body_view;
pub mod cookie_dialog;
mod cookie_form;
mod cookies_tab;
mod error_panel;
mod find;
mod headers_view;
mod hex_view;
mod large_body;
mod pretty;
mod preview;
mod request_as_sent;
mod save_body;
mod status_line;
mod timeline;

pub use body_view::BodyView;
pub use cookies_tab::{CookiesTab, ParsedCookie};
pub use headers_view::{HeaderRow, HeadersView};
pub use save_body::write_body;
pub use status_line::StatusLine;
pub use timeline::TimelineTab;

use std::sync::Arc;

use gpui::Task;
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

/// The debug key the status line renders under — the test hook that proves a response's
/// status line is on screen. It renders only while the view holds a response, so finding
/// it also proves one arrived. `request_editor` re-exports this key for its embedded-view
/// tests (S29e).
pub const STATUS_LINE_SELECTOR: &str = "status-line";

/// The tabs across the top of the response, in order. Later steps append here (Cookies,
/// Timeline); the bar is built from this list, so a new tab is one line and nothing else.
const TAB_TITLES: [&str; 4] = ["Body", "Headers", "Cookies", "Timeline"];
/// Where the body renders.
const BODY_TAB: usize = 0;
/// Where the headers table renders.
const HEADERS_TAB: usize = 1;
/// Where the cookies table renders.
const COOKIES_TAB: usize = 2;
/// Where the timeline renders.
const TIMELINE_TAB: usize = 3;

/// The response region: status line on top, tabs, and the selected pane below.
pub struct ResponseView {
    focus_handle: FocusHandle,
    /// The status line for the response on screen, precomputed when it arrived.
    status_line: Option<StatusLine>,
    tab_bar: Entity<TabBar>,
    selected_tab: usize,
    body: Entity<BodyView>,
    headers: Entity<HeadersView>,
    cookies: Entity<CookiesTab>,
    timeline: Entity<TimelineTab>,
    /// The failed send on screen instead of its body (REQ-RESP-08).
    error: Option<TransportError>,
    /// The response on screen, kept for `response::SaveBody` and `response::CopyBody`
    /// (REQ-RESP-07): the actions save and copy the bytes it arrived as.
    response: Option<Arc<Response>>,
    /// The in-flight save / copy (REQ-RESP-07). Stored so a second one replaces
    /// (cancels) the first (AGENTS.md › Async).
    save_task: Option<Task<()>>,
    copy_task: Option<Task<()>>,
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
            cookies: cx.new(CookiesTab::new),
            timeline: cx.new(TimelineTab::new),
            error: None,
            response: None,
            save_task: None,
            copy_task: None,
            _tab_bar_subscription,
        }
    }

    /// Show this response. It is dismantled once here — the body goes to [`BodyView`]
    /// without a copy, the headers to [`HeadersView`], and the status line precomputes
    /// its texts — so `render` only lays out what arrived.
    pub fn set_response(&mut self, response: Response, cx: &mut Context<Self>) {
        let response = Arc::new(response);
        self.error = None;
        self.response = Some(Arc::clone(&response));
        self.status_line = Some(StatusLine::new(
            ElementId::from(&self.focus_handle),
            response.status,
            response.reason.clone(),
            response.timings.total,
            body_size(&response.body),
        ));
        self.headers
            .update(cx, |headers, cx| headers.set_headers(&response.headers, cx));
        self.cookies
            .update(cx, |cookies, cx| cookies.set_headers(&response.headers, cx));
        self.timeline
            .update(cx, |timeline, cx| timeline.set_response(&response, cx));
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
                COOKIES_TAB => self.cookies.clone().into_any_element(),
                TIMELINE_TAB => self.timeline.clone().into_any_element(),
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
            .on_action(cx.listener(Self::find))
            .on_action(cx.listener(Self::save_body))
            .on_action(cx.listener(Self::copy_body))
            .track_focus(&self.focus_handle)
            .map(|this| match &self.status_line {
                Some(status_line) => this
                    .child(
                        div()
                            .debug_selector(|| STATUS_LINE_SELECTOR.to_owned())
                            .child(status_line.clone()),
                    )
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
/// dependencies). The views are created by their hosts; the app-level actions live
/// with their feature — [`cookie_dialog`] answers `cookies::Manage` (REQ-HTTP-08).
pub fn init(cx: &mut App) {
    cookie_dialog::init(cx);
}

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
