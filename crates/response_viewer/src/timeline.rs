//! The Timeline tab: what a send took and where it went — the request as it went on the
//! wire (REQ-NET-04, [`request_as_sent`]), the measured phases as bars (REQ-NET-01), the
//! network and TLS facts behind the connection (REQ-NET-02) and every hop of the
//! redirect chain with its response headers behind a toggle (REQ-NET-03).
//!
//! `Decision:` ADR-0003 decides what may appear. Only a phase the transport measured gets
//! a bar; the four it cannot measure draw no bar at all — never a zero bar — and are named
//! in [`UNMEASURED_NOTE`] ("absent rather than zeroed"). A network fact nobody observed
//! reads "not reported" instead of a value.
//!
//! `Decision:` an expanded hop's response headers are lines of the same [`VirtualList`] as
//! the hop summaries, so every line of the list stays one line tall. A hop's identity is
//! its position in the chain, which is immutable per response, so index-derived ids are
//! stable here.
//!
//! Hop response headers can carry cookies and auth material (PRD §7): the row types that
//! hold them have no [`Debug`](std::fmt::Debug) and nothing here logs them.
//!
//! The rows are a snapshot taken when the response arrives ([`TimelineTab::set_response`],
//! or a hop's toggle for the expanded headers); render only lays them out. Bar widths are
//! derived in [`TimelineTab::set_response`], never in `render`.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use api_model::SecretValues;
use gpui::{DefiniteLength, WeakEntity};
use http_engine::{
    Certificate, NetworkInfo, RedirectHop, ResolvedRequest, Response, ResponseVersion, Timings,
};
use ui::prelude::*;

use crate::child_id;
use crate::request_as_sent::{self, RequestAsSentRow};

/// Shown for a fact nobody observed: an absent `Location`, an unmeasured hop timing.
const ABSENT: &str = "—";

/// Shown instead of a network fact the transport cannot report (ADR-0003): the viewer
/// says so instead of showing a value nobody observed.
const NOT_REPORTED: &str = "not reported";

/// The note under the phases (ADR-0003): which phases this transport cannot measure, and
/// that they are absent rather than zeroed (REQ-NET-01).
const UNMEASURED_NOTE: &str = "Prepare, DNS lookup, TCP connect and TLS handshake: this \
    transport cannot measure them, so they are absent rather than zeroed (ADR-0003).";

/// The label column of the label-and-value rows. A fixed column keeps the bars and the
/// values aligned row over row while the labels truncate inside it.
const LABEL_COLUMN_PX: f32 = 144.0;

/// The narrow columns of a hop row: its method, its status and its timing.
const METHOD_COLUMN_PX: f32 = 64.0;
const STATUS_COLUMN_PX: f32 = 56.0;
const TIMING_COLUMN_PX: f32 = 72.0;

/// One phase definition: its stable slug, the label the row shows and where the transport
/// records it.
type PhaseDefinition = (&'static str, &'static str, fn(&Timings) -> Option<Duration>);

/// One measured phase, as [`phase_rows`] carries it to the rows it derives.
type MeasuredPhase = (&'static str, &'static str, Duration);

/// Every phase of REQ-NET-01 in waterfall order: its stable slug, the label the row shows
/// and where the transport records it. A phase whose value is `None` gets no row at all —
/// [`phase_rows`] decides that.
const PHASES: [PhaseDefinition; 7] = [
    ("prepare", "Prepare", |timings| timings.prepare),
    ("dns", "DNS lookup", |timings| timings.dns),
    ("connect", "TCP connect", |timings| timings.connect),
    ("tls", "TLS handshake", |timings| timings.tls),
    ("first-byte", "Time to first byte", |timings| {
        timings.first_byte
    }),
    ("download", "Download", |timings| timings.download),
    ("total", "Total", |timings| timings.total),
];

/// One measured phase as the phases section draws it: its label, its time and the bar
/// width as a fraction of the whole exchange — all derived in
/// [`TimelineTab::set_response`], never in `render`.
#[derive(Clone, Debug)]
pub struct PhaseRow {
    pub label: SharedString,
    pub time: SharedString,
    /// The bar's width as a fraction of the exchange, clamped to `0.0..=1.0`.
    pub width: f32,
    /// The phase's stable slug, which element ids and test selectors are built from.
    pub slug: &'static str,
}

/// One fact of the network section: what it is called, what the transport reported, and
/// whether it reported anything at all.
#[derive(Clone, Debug)]
pub struct InfoRow {
    pub label: SharedString,
    pub value: SharedString,
    /// Whether the transport observed this fact. An unobserved one reads
    /// [`NOT_REPORTED`] (ADR-0003).
    pub reported: bool,
}

/// One redirect hop as the list draws it: the summary facts precomputed when the response
/// arrived, and the hop's response headers for its expandable detail.
///
/// No [`Debug`](std::fmt::Debug): `headers` can carry cookies and auth material (PRD §7).
#[derive(Clone)]
pub struct HopRow {
    pub method: SharedString,
    pub url: SharedString,
    pub status: SharedString,
    /// The `Location` this hop answered with, or [`ABSENT`] when it answered without one.
    pub location: SharedString,
    pub timing: SharedString,
    /// The hop's response headers in order, shown when the hop is expanded.
    pub headers: Vec<(SharedString, SharedString)>,
}

/// One line of the redirect list: a hop's summary, or — under an expanded hop — one of its
/// response headers. Expanded headers become lines of the same uniform list, so every
/// line stays one line tall.
enum RedirectRow {
    Hop(usize),
    Header { hop: usize, header: usize },
}

/// The Timeline tab: the derived rows of one response and the list that draws them.
pub struct TimelineTab {
    /// The "Request as sent" record of the send (REQ-NET-04), or `None` before one was
    /// recorded — then the section does not render at all.
    request_as_sent: Option<Arc<Vec<RequestAsSentRow>>>,
    /// The measured phases, in waterfall order.
    phases: Arc<Vec<PhaseRow>>,
    /// The network and TLS facts of the send.
    network: Arc<Vec<InfoRow>>,
    /// The redirect chain, hop by hop.
    hops: Arc<Vec<HopRow>>,
    /// The lines the redirect list draws: the hop summaries plus the headers of the
    /// expanded hops.
    rows: Arc<Vec<RedirectRow>>,
    /// Which hops are expanded. Never iterated for drawing — only asked — so the list's
    /// order comes from the chain.
    expanded: HashSet<usize>,
    /// One focus handle per hop for its expand toggle — a handle made in render would be
    /// new every frame and drop the toggle's focus with it (`headers_view`).
    hop_focus: Arc<Vec<FocusHandle>>,
}

impl TimelineTab {
    /// An empty tab, waiting for a response.
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            request_as_sent: None,
            phases: Arc::new(Vec::new()),
            network: Arc::new(Vec::new()),
            hops: Arc::new(Vec::new()),
            rows: Arc::new(Vec::new()),
            expanded: HashSet::new(),
            hop_focus: Arc::new(Vec::new()),
        }
    }

    /// Show what this response recorded. Every row is derived here (never in `render`):
    /// the phases the transport measured, the facts it reported and the redirect chain.
    pub fn set_response(&mut self, response: &Response, cx: &mut Context<Self>) {
        self.phases = Arc::new(phase_rows(&response.timings));
        self.network = Arc::new(network_rows(response));
        self.hops = Arc::new(hop_rows(&response.redirects));
        self.expanded.clear();
        self.rows = Arc::new(redirect_rows(&self.hops, &self.expanded));
        self.hop_focus = Arc::new(
            self.hops
                .iter()
                .map(|_| cx.focus_handle().tab_stop(true))
                .collect(),
        );
        cx.notify();
    }

    /// Record the request as it went on the wire (REQ-NET-04): the final URL, the
    /// method, every header and a bounded body preview, every value redacted
    /// (`request_as_sent`). `cookie_header` is the `Cookie` header the jar applied to
    /// the send, as `CookieJar::cookie_header_for` computed it — the transport applies
    /// it after this request was recorded, so it is not among `request`'s headers.
    ///
    /// Who calls this from the send path is later wiring (`Decision:`,
    /// [`request_as_sent`]); [`Self::set_response`] is unchanged.
    pub fn set_request_as_sent(
        &mut self,
        request: &ResolvedRequest,
        secrets: &SecretValues,
        cookie_header: Option<String>,
        response: &Response,
        cx: &mut Context<Self>,
    ) {
        self.request_as_sent = Some(Arc::new(request_as_sent::rows(
            request,
            secrets,
            cookie_header.as_deref(),
            response,
        )));
        cx.notify();
    }

    /// The "Request as sent" rows the section draws — what `render` lays out and what
    /// the tests assert on. Empty until [`Self::set_request_as_sent`] records one.
    pub fn request_as_sent_rows(&self) -> &[RequestAsSentRow] {
        self.request_as_sent.as_deref().map_or(&[], Vec::as_slice)
    }

    /// The phase rows the section draws — what `render` lays out and what the tests
    /// assert on.
    pub fn phase_rows(&self) -> &[PhaseRow] {
        &self.phases
    }

    /// The hop rows the list draws — what `render` lays out and what the tests assert on.
    pub fn hop_rows(&self) -> &[HopRow] {
        &self.hops
    }

    /// Show (or hide) one hop's response headers in the list. The rows are rebuilt here,
    /// so render stays a pure layout of what this snapshot holds.
    fn toggle_hop(&mut self, hop: usize, cx: &mut Context<Self>) {
        if !self.expanded.remove(&hop) {
            self.expanded.insert(hop);
        }
        self.rows = Arc::new(redirect_rows(&self.hops, &self.expanded));
        cx.notify();
    }

    /// The "Request as sent" section (REQ-NET-04): the final URL, the method, every
    /// header and a bounded body preview (`request_as_sent`), in its own virtualized
    /// list — the header rows are user data and can be many. The list shares the tab's
    /// free height with the redirect list.
    ///
    /// `Decision:` the section sits at the top of the tab: the request is the story the
    /// phases, the network facts and the redirects tell afterwards. With no record it
    /// renders nothing at all, so the tab stays the S31a timeline until the send path
    /// calls [`Self::set_request_as_sent`].
    fn request_as_sent_section(&self, base: &ElementId, cx: &App) -> Option<AnyElement> {
        let rows = Arc::clone(self.request_as_sent.as_ref()?);
        let key_rows = Arc::clone(&rows);
        let key_base = base.clone();
        let render_base = base.clone();
        Some(
            v_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .gap(DynamicSpacing::Base02.rems(cx))
                .child(section_heading("Request as sent"))
                .child(VirtualList::new(
                    child_id(base, "request-as-sent-rows"),
                    rows.len(),
                    move |index| request_row_id(&key_rows, &key_base, index),
                    move |index, _window, cx| render_request_row(&rows, &render_base, index, cx),
                ))
                .into_any_element(),
        )
    }

    /// The phases section: one row per measured phase, the bars, and the note that names
    /// the four this transport cannot measure.
    fn phases_section(&self, cx: &App) -> AnyElement {
        v_flex()
            .w_full()
            .gap(DynamicSpacing::Base02.rems(cx))
            .child(section_heading("Phases"))
            .children(self.phases.iter().map(|row| render_phase(row, cx)))
            .child(
                div()
                    .debug_selector(|| "timeline-unmeasured-note".to_owned())
                    .child(
                        Label::new(UNMEASURED_NOTE)
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .into_any_element()
    }

    /// The network section: every fact of REQ-NET-02, "not reported" where the transport
    /// observed nothing.
    fn network_section(&self, base: &ElementId, cx: &App) -> AnyElement {
        v_flex()
            .w_full()
            .gap(DynamicSpacing::Base02.rems(cx))
            .child(section_heading("Network"))
            .children(
                self.network
                    .iter()
                    .map(|row| render_info_row(row, base, cx)),
            )
            .into_any_element()
    }

    /// The redirect section: every hop of the chain, virtualized like the headers table,
    /// with each hop's response headers behind its toggle.
    fn redirects_section(&self, base: &ElementId, cx: &mut Context<Self>) -> AnyElement {
        let rows = Arc::clone(&self.rows);
        let hops = Arc::clone(&self.hops);
        let hop_focus = Arc::clone(&self.hop_focus);
        let expanded = self.expanded.clone();
        let view = cx.weak_entity();
        let key_base = base.clone();
        let render_base = base.clone();
        v_flex()
            .w_full()
            .flex_1()
            .min_h_0()
            .gap(DynamicSpacing::Base02.rems(cx))
            .child(section_heading("Redirect chain"))
            .map(|this| {
                if self.hops.is_empty() {
                    this.child(empty_state())
                } else {
                    let key_rows = Arc::clone(&rows);
                    this.child(VirtualList::new(
                        child_id(base, "redirect-rows"),
                        rows.len(),
                        move |index| row_id_for(&key_rows, &key_base, index),
                        move |index, _window, cx| {
                            let list = RedirectList {
                                rows: &rows,
                                hops: &hops,
                                hop_focus: &hop_focus,
                                expanded: &expanded,
                                base: &render_base,
                                view: &view,
                            };
                            render_redirect_line(&list, index, cx)
                        },
                    ))
                }
            })
            .into_any_element()
    }
}

impl Render for TimelineTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One base id per entity, so two viewers in one frame never share element ids.
        let base = ElementId::from(("timeline-tab", cx.entity_id()));
        let request_as_sent = self.request_as_sent_section(&base, cx);
        let phases = self.phases_section(cx);
        let network = self.network_section(&base, cx);
        let redirects = self.redirects_section(&base, cx);
        v_flex()
            .size_full()
            .px(DynamicSpacing::Base04.rems(cx))
            .py(DynamicSpacing::Base03.rems(cx))
            .gap(DynamicSpacing::Base06.rems(cx))
            .children(request_as_sent)
            .child(phases)
            .child(network)
            .child(redirects)
    }
}

/// Everything a redirect line's renderer needs: the snapshot and where each line sits.
struct RedirectList<'a> {
    rows: &'a [RedirectRow],
    hops: &'a [HopRow],
    hop_focus: &'a [FocusHandle],
    expanded: &'a HashSet<usize>,
    base: &'a ElementId,
    view: &'a WeakEntity<TimelineTab>,
}

/// The phase rows of one response: only the phases the transport measured (REQ-NET-01,
/// ADR-0003), each bar as a fraction of the whole exchange. Pure: no IO, no view state.
fn phase_rows(timings: &Timings) -> Vec<PhaseRow> {
    let measured: Vec<MeasuredPhase> = PHASES
        .iter()
        .filter_map(|(slug, label, duration)| Some((*slug, *label, duration(timings)?)))
        .collect();
    let scale = bar_scale(timings, &measured);
    measured
        .into_iter()
        .map(|(slug, label, duration)| PhaseRow {
            label: label.into(),
            time: format_duration(Some(duration)),
            width: bar_fraction(duration, scale),
            slug,
        })
        .collect()
}

/// What the bars are measured against: the whole exchange when the transport recorded it,
/// otherwise the longest phase it did — so the widest bar still fills the track.
fn bar_scale(timings: &Timings, measured: &[MeasuredPhase]) -> Duration {
    timings
        .total
        .or_else(|| measured.iter().map(|(_, _, duration)| *duration).max())
        .unwrap_or_default()
}

/// A phase's bar width as a fraction of `scale`, clamped to the track. Nothing is cast
/// with `as`, and a zero scale draws a zero bar rather than a NaN one.
fn bar_fraction(duration: Duration, scale: Duration) -> f32 {
    let scale = scale.as_secs_f32();
    if scale <= 0.0 {
        return 0.0;
    }
    (duration.as_secs_f32() / scale).clamp(0.0, 1.0)
}

/// The network rows of one response (REQ-NET-02): every fact, and "not reported" for one
/// the transport could not observe (ADR-0003). Pure: no IO, no view state.
fn network_rows(response: &Response) -> Vec<InfoRow> {
    let network = response.network.as_ref();
    let tls = network.and_then(|network| network.tls.as_ref());
    let mut rows = vec![
        info_row("HTTP version", Some(http_version(response.http_version))),
        info_row("Remote address", remote_address(network)),
        info_row(
            "Local address",
            network
                .and_then(|network| network.local_address.clone())
                .map(SharedString::from),
        ),
        info_row(
            "ALPN",
            network
                .and_then(|network| network.alpn.clone())
                .map(SharedString::from),
        ),
        info_row(
            "TLS protocol",
            tls.and_then(|tls| tls.protocol.clone())
                .map(SharedString::from),
        ),
        info_row(
            "Cipher",
            tls.and_then(|tls| tls.cipher.clone())
                .map(SharedString::from),
        ),
    ];
    match tls.and_then(|tls| tls.certificates.as_deref()) {
        Some(chain) if !chain.is_empty() => {
            rows.extend(chain.iter().enumerate().map(|(index, certificate)| {
                info_row(
                    &format!("Certificate {}", index + 1),
                    Some(certificate_text(certificate).into()),
                )
            }))
        }
        _ => rows.push(info_row("Certificate chain", None)),
    }
    rows.push(info_row(
        "TLS verification",
        tls.map(|tls| {
            SharedString::from(if tls.verification_enabled {
                "On"
            } else {
                "Off"
            })
        }),
    ));
    rows
}

/// One network fact: a reported value, or [`NOT_REPORTED`] where there was none.
fn info_row(label: &str, value: Option<SharedString>) -> InfoRow {
    match value {
        Some(value) if !value.is_empty() => InfoRow {
            label: label.into(),
            value,
            reported: true,
        },
        _ => InfoRow {
            label: label.into(),
            value: NOT_REPORTED.into(),
            reported: false,
        },
    }
}

/// The remote endpoint as one address: `ip:port`, or whichever half the transport saw.
fn remote_address(network: Option<&NetworkInfo>) -> Option<SharedString> {
    let network = network?;
    match (&network.remote_ip, network.remote_port) {
        (Some(ip), Some(port)) => Some(format!("{ip}:{port}").into()),
        (Some(ip), None) => Some(ip.as_str().into()),
        (None, Some(port)) => Some(port.to_string().into()),
        (None, None) => None,
    }
}

/// The HTTP version the response arrived on, as REQ-NET-02 names it.
fn http_version(version: ResponseVersion) -> SharedString {
    match version {
        ResponseVersion::Http10 => "HTTP/1.0",
        ResponseVersion::Http11 => "HTTP/1.1",
        ResponseVersion::Http2 => "HTTP/2",
        ResponseVersion::Http3 => "HTTP/3",
    }
    .into()
}

/// Every field REQ-NET-02 names for one certificate of the peer chain.
fn certificate_text(certificate: &Certificate) -> String {
    let subject_alt_names = if certificate.subject_alt_names.is_empty() {
        ABSENT.to_owned()
    } else {
        certificate.subject_alt_names.join(", ")
    };
    format!(
        "subject {}; issuer {}; valid {} to {}; SANs {}; SHA-256 {}",
        certificate.subject,
        certificate.issuer,
        certificate.valid_from,
        certificate.valid_until,
        subject_alt_names,
        certificate.sha256_fingerprint,
    )
}

/// The hop rows of one chain (REQ-NET-03), in the order the hops happened. Pure: no IO,
/// no view state.
fn hop_rows(hops: &[RedirectHop]) -> Vec<HopRow> {
    hops.iter()
        .map(|hop| HopRow {
            method: hop.method.as_str().into(),
            url: hop.url.as_str().into(),
            status: hop.status.to_string().into(),
            location: match hop.location.as_ref() {
                Some(location) => location.as_str().into(),
                None => ABSENT.into(),
            },
            timing: format_duration(hop.timing),
            headers: hop
                .headers
                .iter()
                .map(|header| (header.name.as_str().into(), header.value.as_str().into()))
                .collect(),
        })
        .collect()
}

/// The lines the redirect list draws: every hop's summary, then — under each expanded
/// hop — its response headers, one line each.
fn redirect_rows(hops: &[HopRow], expanded: &HashSet<usize>) -> Vec<RedirectRow> {
    let mut rows = Vec::new();
    for (hop, row) in hops.iter().enumerate() {
        rows.push(RedirectRow::Hop(hop));
        if expanded.contains(&hop) {
            rows.extend((0..row.headers.len()).map(|header| RedirectRow::Header { hop, header }));
        }
    }
    rows
}

/// One redirect line's stable element id under `base`: the hop's position in the chain,
/// and which header of it the line shows — the chain is immutable per response, so these
/// never shift.
fn row_id_for(rows: &[RedirectRow], base: &ElementId, index: usize) -> ElementId {
    match rows.get(index) {
        Some(RedirectRow::Hop(hop)) => child_id(base, format!("hop-row-{hop}")),
        Some(RedirectRow::Header { hop, header }) => {
            child_id(base, format!("hop-{hop}-header-{header}"))
        }
        None => child_id(base, format!("row-{index}")),
    }
}

/// One line of the redirect list: a hop's summary or one expanded header.
fn render_redirect_line(list: &RedirectList, index: usize, cx: &App) -> AnyElement {
    match list.rows.get(index) {
        Some(RedirectRow::Hop(hop)) => render_hop_line(list, *hop, cx),
        Some(RedirectRow::Header { hop, header }) => render_header_line(list, *hop, *header, cx),
        None => div().into_any_element(),
    }
}

/// One hop row: method, URL, status, the `Location` that was followed and the timing,
/// each truncating with its full value in a tooltip — and the toggle that expands the
/// hop's response headers.
fn render_hop_line(list: &RedirectList, hop: usize, cx: &App) -> AnyElement {
    let (Some(row), Some(focus_handle)) = (list.hops.get(hop), list.hop_focus.get(hop)) else {
        return div().into_any_element();
    };
    let row_id = child_id(list.base, format!("hop-row-{hop}"));
    h_flex()
        .id(row_id.clone())
        .w_full()
        .items_center()
        .gap(DynamicSpacing::Base04.rems(cx))
        .px(DynamicSpacing::Base04.rems(cx))
        .py(DynamicSpacing::Base02.rems(cx))
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .debug_selector(move || format!("timeline-hop-{hop}"))
        .child(expand_toggle(
            child_id(&row_id, "toggle"),
            hop,
            list.expanded.contains(&hop),
            focus_handle.clone(),
            list.view,
        ))
        .child(fixed_cell(
            child_id(&row_id, "method"),
            row.method.clone(),
            METHOD_COLUMN_PX,
            cx,
        ))
        .child(cell(child_id(&row_id, "url"), row.url.clone(), cx))
        .child(fixed_cell(
            child_id(&row_id, "status"),
            row.status.clone(),
            STATUS_COLUMN_PX,
            cx,
        ))
        .child(cell(
            child_id(&row_id, "location"),
            row.location.clone(),
            cx,
        ))
        .child(fixed_cell(
            child_id(&row_id, "timing"),
            row.timing.clone(),
            TIMING_COLUMN_PX,
            cx,
        ))
        .into_any_element()
}

/// One header line under an expanded hop: name and value, as the hop answered.
fn render_header_line(list: &RedirectList, hop: usize, header: usize, cx: &App) -> AnyElement {
    let Some((name, value)) = list.hops.get(hop).and_then(|row| row.headers.get(header)) else {
        return div().into_any_element();
    };
    let row_id = child_id(list.base, format!("hop-{hop}-header-{header}"));
    h_flex()
        .id(row_id.clone())
        .w_full()
        .gap(DynamicSpacing::Base04.rems(cx))
        .px(DynamicSpacing::Base04.rems(cx))
        .py(DynamicSpacing::Base02.rems(cx))
        .pl(DynamicSpacing::Base24.rems(cx))
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .debug_selector(move || format!("timeline-hop-{hop}-header-{header}"))
        .child(cell(child_id(&row_id, "name"), name.clone(), cx))
        .child(cell(child_id(&row_id, "value"), value.clone(), cx))
        .into_any_element()
}

/// The hop's expand toggle: a `IconButton`, so the toggle is a tab stop and Enter or
/// Space on it fires the same handler a click does (`ui`'s `Clickable`). The tooltip is
/// its accessible label and says what activating it does.
fn expand_toggle(
    id: ElementId,
    hop: usize,
    expanded: bool,
    focus_handle: FocusHandle,
    view: &WeakEntity<TimelineTab>,
) -> impl IntoElement {
    let (icon, label) = if expanded {
        (IconName::ChevronDown, "Hide response headers")
    } else {
        (IconName::ChevronRight, "Show response headers")
    };
    let view = view.clone();
    IconButton::new(id, icon, Tooltip::text(label))
        .size(ButtonSize::Default)
        .focus_handle(focus_handle)
        .on_click(move |_event, _window, cx| {
            if view.update(cx, |tab, cx| tab.toggle_hop(hop, cx)).is_err() {
                // Handled: the tab went away with its window while the toggle was up;
                // there is nothing left to expand.
            }
        })
}

/// One "Request as sent" row's stable element id: its place in the record and its
/// label. The record is rebuilt only when a new send is recorded, so these never shift
/// under a live list (`Decision:`, [`request_as_sent`]).
fn request_row_id(rows: &[RequestAsSentRow], base: &ElementId, index: usize) -> ElementId {
    match rows.get(index) {
        Some(row) => child_id(base, format!("request-row-{index}-{}", row.label)),
        None => child_id(base, format!("request-row-{index}")),
    }
}

/// One "Request as sent" row: its label and the redacted value beside it — truncated,
/// with the full value in a tooltip — and, for a header the body mode generated on its
/// own, the `auto` mark.
fn render_request_row(
    rows: &[RequestAsSentRow],
    base: &ElementId,
    index: usize,
    cx: &App,
) -> AnyElement {
    let Some(row) = rows.get(index) else {
        return div().into_any_element();
    };
    let row_id = request_row_id(rows, base, index);
    h_flex()
        .w_full()
        .items_center()
        .gap(DynamicSpacing::Base04.rems(cx))
        .px(DynamicSpacing::Base04.rems(cx))
        .py(DynamicSpacing::Base02.rems(cx))
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .child(label_cell(row.label.clone()))
        .child(cell(child_id(&row_id, "value"), row.value.clone(), cx))
        .map(|this| {
            if row.auto {
                this.child(auto_mark())
            } else {
                this
            }
        })
        .into_any_element()
}

/// The `auto` mark beside a header the body mode generated on its own (REQ-HTTP-03).
/// `ui`'s `Badge` is text-first, so the mark says what it is instead of only tinting
/// (`REQ-NFR-06`).
fn auto_mark() -> impl IntoElement {
    Badge::new("auto")
}

/// The label column of a label-and-value row (phase, network fact or request row).
fn label_cell(text: SharedString) -> impl IntoElement {
    div()
        .flex_none()
        .w(px_to_rems(LABEL_COLUMN_PX))
        .min_w_0()
        .child(
            Label::new(text)
                .size(LabelSize::Small)
                .color(Color::Muted)
                .truncate(),
        )
}

/// One measured phase: its label, its bar and the time beside it. A measured phase draws
/// its bar; a phase the transport did not measure has no row and draws nothing.
fn render_phase(row: &PhaseRow, cx: &App) -> impl IntoElement {
    let slug = row.slug;
    h_flex()
        .w_full()
        .items_center()
        .gap(DynamicSpacing::Base04.rems(cx))
        .child(label_cell(row.label.clone()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(DynamicSpacing::Base02.rems(cx))
                .rounded_sm()
                .bg(cx.theme().colors().element_background)
                .child(
                    div()
                        .h_full()
                        .w(DefiniteLength::Fraction(row.width))
                        .rounded_sm()
                        .bg(cx.theme().colors().accent_background)
                        .debug_selector(move || format!("timeline-bar-{slug}")),
                ),
        )
        .child(
            Label::new(row.time.clone())
                .size(LabelSize::Small)
                .color(Color::Muted),
        )
}

/// One network fact: its label and its value, an unobserved one marked [`NOT_REPORTED`]
/// and muted so absence reads as absence.
fn render_info_row(row: &InfoRow, base: &ElementId, cx: &App) -> impl IntoElement {
    let row_id = child_id(base, format!("network-{}", row.label));
    let value_color = if row.reported {
        cx.theme().colors().text
    } else {
        cx.theme().colors().text_muted
    };
    h_flex()
        .w_full()
        .items_center()
        .gap(DynamicSpacing::Base04.rems(cx))
        .child(label_cell(row.label.clone()))
        .child(
            div()
                .id(child_id(&row_id, "value"))
                .flex_1()
                .min_w_0()
                .font_buffer(cx)
                .text_ui_sm(cx)
                .text_color(value_color)
                .tooltip(Tooltip::text(row.value.clone()).builder())
                .child(div().w_full().min_w_0().truncate().child(row.value.clone())),
        )
}

/// A fixed-width cell beside the flexible ones, so the columns line up row over row.
fn fixed_cell(id: ElementId, text: SharedString, width: f32, cx: &App) -> impl IntoElement {
    div()
        .flex_none()
        .w(px_to_rems(width))
        .min_w_0()
        .child(cell(id, text, cx))
}

/// One cell: the content font, truncated with the full value behind a tooltip (the
/// `headers_view` cell; that helper is private to a file another step owns, so this is
/// the same shape rather than a call — `cookies_tab::cell`'s precedent).
fn cell(id: ElementId, text: SharedString, cx: &App) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .font_buffer(cx)
        .text_ui_sm(cx)
        .text_color(cx.theme().colors().text)
        .tooltip(Tooltip::text(text.clone()).builder())
        .child(div().w_full().min_w_0().truncate().child(text))
}

/// A section heading above its rows (`LabelSize::Large` is the design system's section
/// title).
fn section_heading(title: &'static str) -> impl IntoElement {
    Label::new(title).size(LabelSize::Large)
}

/// The state where there is nothing to list: a response that answered without redirecting.
fn empty_state() -> impl IntoElement {
    v_flex()
        .size_full()
        .justify_center()
        .items_center()
        .child(Label::new("No redirects in this response").color(Color::Muted))
}

/// A duration as the rows show it: milliseconds under a second, seconds above one,
/// minutes and seconds for anything over a minute (the `status_line` shape; that helper
/// is private to a file another step owns, so this is the same shape rather than a call).
fn format_duration(duration: Option<Duration>) -> SharedString {
    let Some(duration) = duration else {
        return ABSENT.into();
    };
    if duration < Duration::from_secs(1) {
        format!("{:.0} ms", duration.as_secs_f64() * 1000.0)
    } else if duration < Duration::from_secs(60) {
        format!("{:.2} s", duration.as_secs_f64())
    } else {
        format!(
            "{} m {:02} s",
            duration.as_secs() / 60,
            duration.as_secs() % 60
        )
    }
    .into()
}

#[cfg(test)]
#[path = "timeline_tests.rs"]
mod tests;
