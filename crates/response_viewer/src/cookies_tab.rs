//! The Cookies tab: the response's `Set-Cookie` headers, parsed, one row per cookie with
//! every attribute (REQ-RESP-02, REQ-HTTP-08).
//!
//! [`parse_set_cookies`] is a pure function over the response's headers — the `cookie`
//! crate (through `cookie_store`'s [`RawCookie`]) does the RFC 6265 parsing; nothing here
//! re-implements it. The rows are a snapshot taken when the response arrives
//! ([`CookiesTab::set_headers`]); render lays them out through [`VirtualList`], the same
//! shape as `headers_view`.
//!
//! `Decision:` a `Set-Cookie` header the `cookie` crate cannot parse is never dropped —
//! its raw text renders in the row's Name cell and the Value cell reads "unparseable", so
//! the user still sees exactly what came over the wire.
//!
//! `Decision:` the nine columns share the row width evenly, like `headers_view`'s two.
//! Each cell truncates with its full text in a tooltip, so nothing is lost to the layout.
//!
//! Cookie values are secrets (PRD §7): nothing here logs or debug-prints them, and
//! [`ParsedCookie`]'s [`Debug`](fmt::Debug) redacts the value — and, for unparseable rows,
//! the raw header text their name holds.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use cookie_store::RawCookie;
use http_engine::Header;
use pustman_actions::cookies::Manage;
use time::format_description::well_known::Rfc2822;
use ui::prelude::*;

use crate::child_id;

/// Shown where a cookie attribute is absent.
const ABSENT: &str = "—";

/// Shown instead of the attributes when a `Set-Cookie` header does not parse.
const UNPARSEABLE: &str = "unparseable";

/// Shown by [`Debug`](fmt::Debug) where a row's name holds an unparseable header's raw
/// text: that text embeds the cookie value (PRD §7), so it never reaches logs or test
/// output — only the UI cell.
const UNPARSEABLE_REDACTED: &str = "<unparseable-redacted>";

/// The columns the table shows, in order: the cell's id (element identity) and its
/// heading. [`ParsedCookie::cells`] fills them in the same order.
const COLUMNS: [(&str, &str); 9] = [
    ("name", "Name"),
    ("value", "Value"),
    ("domain", "Domain"),
    ("path", "Path"),
    ("expires", "Expires"),
    ("max-age", "Max-Age"),
    ("secure", "Secure"),
    ("http-only", "HttpOnly"),
    ("same-site", "SameSite"),
];

/// One `Set-Cookie` header as the tab shows it: the attributes precomputed into display
/// strings when the response arrived, so `render` only lays them out.
#[derive(Clone)]
pub struct ParsedCookie {
    /// The cookie name — or the header's raw text when it does not parse.
    pub name: SharedString,
    /// The cookie value. A secret (PRD §7): [`Debug`](fmt::Debug) prints `<redacted>`.
    pub value: SharedString,
    pub domain: SharedString,
    pub path: SharedString,
    pub expires: SharedString,
    pub max_age: SharedString,
    pub secure: SharedString,
    pub http_only: SharedString,
    pub same_site: SharedString,
    /// Whether the header failed to parse: its row shows the raw text and
    /// [`UNPARSEABLE`] instead of the attributes.
    pub unparseable: bool,
    /// Which occurrence of `name` this cookie is: the stable identity element ids are
    /// built from, so duplicate names keep their own row identity (`headers_view`).
    occurrence: usize,
}

impl ParsedCookie {
    /// Parse one raw `Set-Cookie` header value. Anything `cookie` refuses becomes a row
    /// marked [`Self::unparseable`], keeping the raw text.
    fn parse(raw: &str) -> Self {
        match RawCookie::parse(raw) {
            Ok(cookie) => Self {
                name: cookie.name().into(),
                value: cookie.value().into(),
                domain: optional(cookie.domain()),
                path: optional(cookie.path()),
                expires: match cookie.expires_datetime() {
                    Some(when) => match when.format(&Rfc2822) {
                        Ok(text) => text.into(),
                        // Formatting a just-parsed date never fails in practice; the
                        // attribute still shows as absent rather than the row vanishing.
                        Err(_) => ABSENT.into(),
                    },
                    // Neither an expiry date nor a Max-Age: the cookie dies with the
                    // session (RFC 6265), so it is not a persistent cookie.
                    None if cookie.max_age().is_none() => "Session".into(),
                    None => ABSENT.into(),
                },
                max_age: match cookie.max_age() {
                    Some(age) => age.whole_seconds().to_string().into(),
                    None => ABSENT.into(),
                },
                secure: flag(cookie.secure()),
                http_only: flag(cookie.http_only()),
                same_site: match cookie.same_site() {
                    Some(site) => site.to_string().into(),
                    None => ABSENT.into(),
                },
                unparseable: false,
                occurrence: 0,
            },
            Err(_) => Self {
                name: raw.into(),
                value: UNPARSEABLE.into(),
                domain: ABSENT.into(),
                path: ABSENT.into(),
                expires: ABSENT.into(),
                max_age: ABSENT.into(),
                secure: ABSENT.into(),
                http_only: ABSENT.into(),
                same_site: ABSENT.into(),
                unparseable: true,
                occurrence: 0,
            },
        }
    }

    /// This row's stable name: the cookie name plus which occurrence of it it is.
    fn key_name(&self) -> SharedString {
        SharedString::from(format!("{}#{}", self.name, self.occurrence))
    }

    /// The name as [`Debug`](fmt::Debug) shows it: an unparseable row's name is the raw
    /// header text — which embeds the cookie value — so it prints as a placeholder while
    /// the UI cell keeps the raw text.
    fn debug_name(&self) -> &str {
        if self.unparseable {
            UNPARSEABLE_REDACTED
        } else {
            &self.name
        }
    }

    /// This row's element identity under `base` — stable across scrolling and independent
    /// of the row's position, so hover and focus never jump to a different cookie.
    fn key(&self, base: &ElementId) -> ElementId {
        child_id(base, self.key_name())
    }

    /// The nine cells the row draws, in the same order as [`COLUMNS`].
    fn cells(&self) -> [SharedString; 9] {
        [
            self.name.clone(),
            self.value.clone(),
            self.domain.clone(),
            self.path.clone(),
            self.expires.clone(),
            self.max_age.clone(),
            self.secure.clone(),
            self.http_only.clone(),
            self.same_site.clone(),
        ]
    }
}

impl fmt::Debug for ParsedCookie {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The value is a secret (PRD §7), and an unparseable row's name holds the raw
        // header text that embeds it: both print as placeholders, never as the bytes
        // that came over the wire.
        f.debug_struct("ParsedCookie")
            .field("name", &self.debug_name())
            .field("value", &"<redacted>")
            .field("domain", &self.domain)
            .field("path", &self.path)
            .field("expires", &self.expires)
            .field("max_age", &self.max_age)
            .field("secure", &self.secure)
            .field("http_only", &self.http_only)
            .field("same_site", &self.same_site)
            .field("unparseable", &self.unparseable)
            .field("occurrence", &self.occurrence)
            .finish()
    }
}

/// Every `Set-Cookie` header in `headers` (name matched case-insensitively), parsed into
/// display rows in the order the headers arrived. Pure: no IO, no view state.
pub fn parse_set_cookies(headers: &[Header]) -> Vec<ParsedCookie> {
    let mut occurrences: HashMap<String, usize> = HashMap::new();
    headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("set-cookie"))
        .map(|header| {
            let mut row = ParsedCookie::parse(&header.value);
            let counter = occurrences.entry(row.name.to_string()).or_default();
            row.occurrence = *counter;
            *counter += 1;
            row
        })
        .collect()
}

/// The Cookies tab: the parsed rows of one response, and the table that draws them.
pub struct CookiesTab {
    rows: Arc<Vec<ParsedCookie>>,
    /// The "Manage cookies" button's focus handle: the tab order the keyboard reaches
    /// the button with (REQ-HTTP-08's dialog opens from here).
    manage_focus: FocusHandle,
}

impl CookiesTab {
    /// An empty tab, waiting for a response.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            rows: Arc::new(Vec::new()),
            manage_focus: cx.focus_handle(),
        }
    }

    /// The "Manage cookies" button, as the keyboard reaches it: the tab hands the
    /// button its tab order through this handle.
    pub fn manage_focus_handle(&self) -> FocusHandle {
        self.manage_focus.clone()
    }

    /// Show the `Set-Cookie` headers of the response that just arrived. The rows are
    /// parsed here (never in `render`), so render stays a pure layout of this snapshot.
    pub fn set_headers(&mut self, headers: &[Header], cx: &mut Context<Self>) {
        self.rows = Arc::new(parse_set_cookies(headers));
        cx.notify();
    }

    /// The rows the tab draws, top to bottom — what `render` lays out and what the tests
    /// assert on.
    pub fn rows(&self) -> &[ParsedCookie] {
        &self.rows
    }
}

impl Render for CookiesTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = Arc::clone(&self.rows);
        // One base id per entity, so two viewers in one frame never share element ids.
        let base = ElementId::from(("cookies-tab", cx.entity_id()));
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_end()
                    .px(DynamicSpacing::Base04.rems(cx))
                    .py(DynamicSpacing::Base02.rems(cx))
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .child(
                        // The tab's one door into the Manage Cookies dialog
                        // (REQ-HTTP-08): it dispatches `cookies::Manage`, whose
                        // app-level handler opens the dialog over this window.
                        Button::new(child_id(&base, "manage-cookies"), "Manage cookies")
                            .size(ButtonSize::Default)
                            .focus_handle(self.manage_focus.clone())
                            .on_click(cx.listener(|_this, _event, window, cx| {
                                window.dispatch_action(Box::new(Manage), cx);
                            })),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .px(DynamicSpacing::Base04.rems(cx))
                    .py(DynamicSpacing::Base02.rems(cx))
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .children(
                        COLUMNS
                            .iter()
                            .map(|(_column, heading)| column_heading(heading)),
                    ),
            )
            .map(|this| {
                if rows.is_empty() {
                    this.child(empty_state())
                } else {
                    let key_rows = Arc::clone(&rows);
                    let key_base = base.clone();
                    let render_base = base.clone();
                    this.child(VirtualList::new(
                        child_id(&base, "rows"),
                        rows.len(),
                        move |index| match key_rows.get(index) {
                            Some(row) => row.key(&key_base),
                            None => child_id(&key_base, format!("row-{index}")),
                        },
                        move |index, _window, cx| render_row(&rows, index, &render_base, cx),
                    ))
                }
            })
    }
}

/// One column heading above the rows (`headers_view`'s, same shape).
fn column_heading(heading: &str) -> impl IntoElement {
    div().flex_1().min_w_0().child(
        Label::new(heading)
            .size(LabelSize::XSmall)
            .color(Color::Muted),
    )
}

/// One cookie row: the nine attribute cells, each truncating with its full value in a
/// tooltip.
fn render_row(rows: &[ParsedCookie], index: usize, base: &ElementId, cx: &mut App) -> AnyElement {
    let Some(row) = rows.get(index) else {
        return div().into_any_element();
    };
    let row_id = row.key(base);
    let cells = row.cells();
    h_flex()
        .id(row_id.clone())
        .w_full()
        .gap(DynamicSpacing::Base04.rems(cx))
        .px(DynamicSpacing::Base04.rems(cx))
        .py(DynamicSpacing::Base02.rems(cx))
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .children(
            COLUMNS
                .iter()
                .zip(cells)
                .map(|((column, _heading), text)| cell(child_id(&row_id, *column), text, cx)),
        )
        .into_any_element()
}

/// One table cell: the content font, truncated with the full value behind a tooltip (the
/// `headers_view` cell; that helper is private to a file another step owns, so this is
/// the same shape rather than a call).
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

/// The state where there is nothing to list: a response that set no cookies.
fn empty_state() -> impl IntoElement {
    v_flex()
        .size_full()
        .justify_center()
        .items_center()
        .child(Label::new("No cookies in this response").color(Color::Muted))
}

/// An absent attribute: shown, marked absent — never a blank cell that reads like an
/// empty value.
fn optional(value: Option<&str>) -> SharedString {
    match value {
        Some(value) => value.into(),
        None => ABSENT.into(),
    }
}

/// A boolean attribute (`Secure`, `HttpOnly`): set, explicitly unset, or absent.
fn flag(value: Option<bool>) -> SharedString {
    match value {
        Some(true) => "Yes".into(),
        Some(false) => "No".into(),
        None => ABSENT.into(),
    }
}

#[cfg(test)]
#[path = "cookies_tab_tests.rs"]
mod tests;
