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
//! `Decision:` the columns are sized to what they hold, not shared evenly (#230): the
//! text columns take a weighted share of the row — the value the user reads most gets the
//! most room — and the short ones (Max-Age, the flags, SameSite) are pinned, so nothing
//! is cramped. Each text cell truncates with its full text in a tooltip, so nothing is
//! lost to the layout.
//!
//! `Decision:` a boolean attribute shows as a flag pill (icon and word together,
//! REQ-NFR-06) and an attribute the `Set-Cookie` header did not set leaves its cell
//! blank — a table read by scanning, not one filled with placeholder marks (S05/R05,
//! `docs/postman_redesign_pack/01_specs/design_system.md`). What a copy takes is
//! unchanged: the words the header said, marks included.
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

use gpui::WeakEntity;

use crate::child_id;
use crate::table_cells::{ColumnWidth, cell, column_heading};
use crate::text_selection::{CellSelector, SelectableCells, TEXT_CONTEXT, selection_handlers};

/// The row's mark for a cookie attribute the `Set-Cookie` header did not set. The table
/// leaves such a cell blank (#230); the mark stays in the row's cells, so what a copy
/// takes still says what the header did.
const ABSENT: &str = "—";

/// A boolean attribute's text in the row's cells — and so in what a copy takes — and on
/// its flag pill: set, or explicitly unset.
const FLAG_SET: &str = "Yes";
const FLAG_UNSET: &str = "No";

/// Shown instead of the attributes when a `Set-Cookie` header does not parse.
const UNPARSEABLE: &str = "unparseable";

/// Shown by [`Debug`](fmt::Debug) where a row's name holds an unparseable header's raw
/// text: that text embeds the cookie value (PRD §7), so it never reaches logs or test
/// output — only the UI cell.
const UNPARSEABLE_REDACTED: &str = "<unparseable-redacted>";

/// The columns the table shows, in order. [`ParsedCookie::cells`] fills them in the same
/// order.
const COLUMNS: [Column; 9] = [
    Column {
        id: "name",
        heading: "Name",
        kind: ColumnKind::Text,
        width: ColumnWidth::Share(2.),
    },
    Column {
        id: "value",
        heading: "Value",
        kind: ColumnKind::Text,
        width: ColumnWidth::Share(3.),
    },
    Column {
        id: "domain",
        heading: "Domain",
        kind: ColumnKind::Text,
        width: ColumnWidth::Share(2.),
    },
    Column {
        id: "path",
        heading: "Path",
        kind: ColumnKind::Text,
        width: ColumnWidth::Share(2.),
    },
    Column {
        id: "expires",
        heading: "Expires",
        kind: ColumnKind::Text,
        width: ColumnWidth::Share(2.5),
    },
    Column {
        id: "max-age",
        heading: "Max-Age",
        kind: ColumnKind::Text,
        width: ColumnWidth::Fixed(Rems(NUMBER_COLUMN_REMS)),
    },
    Column {
        id: "secure",
        heading: "Secure",
        kind: ColumnKind::Flag,
        width: ColumnWidth::Fixed(Rems(FLAG_COLUMN_REMS)),
    },
    Column {
        id: "http-only",
        heading: "HttpOnly",
        kind: ColumnKind::Flag,
        width: ColumnWidth::Fixed(Rems(FLAG_COLUMN_REMS)),
    },
    Column {
        id: "same-site",
        heading: "SameSite",
        kind: ColumnKind::Text,
        width: ColumnWidth::Fixed(Rems(ENUM_COLUMN_REMS)),
    },
];

/// The short columns' widths in rems, so they scale with the UI font size (16 px per rem
/// at the default scale, `ui::units::px_to_rems`): a number and its heading, a flag pill
/// and the "HttpOnly" heading, a SameSite enum and its heading.
const NUMBER_COLUMN_REMS: f32 = 4.5;
const FLAG_COLUMN_REMS: f32 = 5.5;
const ENUM_COLUMN_REMS: f32 = 6.;

/// One column of the cookies table: the id its cells paint under, its heading, how it
/// shows its value, and how wide it is.
#[derive(Clone, Copy)]
struct Column {
    /// The cell's id (element identity) under its row.
    id: &'static str,
    heading: &'static str,
    kind: ColumnKind,
    width: ColumnWidth,
}

/// How a column shows its value.
#[derive(Clone, Copy)]
enum ColumnKind {
    /// Text, truncated with the full value in its tooltip.
    Text,
    /// A boolean attribute: a flag pill where the header set it, blank where it did not.
    Flag,
}

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
    /// The hand-made selection over the cells, flattened row by row (nine per row).
    selection: CellSelector,
    /// Where focus lands when a press selects a cell, so copy has a target.
    focus_handle: FocusHandle,
}

impl SelectableCells for CookiesTab {
    fn cell_selector(&mut self) -> &mut CellSelector {
        &mut self.selection
    }
}

impl CookiesTab {
    /// An empty tab, waiting for a response.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            rows: Arc::new(Vec::new()),
            manage_focus: cx.focus_handle(),
            selection: CellSelector::default(),
            focus_handle: cx.focus_handle(),
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
        // Columns tab-separated and rows on their own lines, so a copy pastes as a table.
        let column_count = COLUMNS.len();
        self.selection.set(
            self.rows
                .iter()
                .flat_map(|row| {
                    row.cells()
                        .into_iter()
                        .enumerate()
                        .map(move |(column, text)| {
                            (
                                text,
                                if column + 1 == column_count {
                                    "\n"
                                } else {
                                    "\t"
                                },
                            )
                        })
                })
                .collect(),
        );
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
        let selection = Arc::new(self.selection.clone());
        let view = cx.weak_entity();
        // One base id per entity, so two viewers in one frame never share element ids.
        let base = ElementId::from(("cookies-tab", cx.entity_id()));
        let root = v_flex()
            .size_full()
            .key_context(TEXT_CONTEXT)
            .track_focus(&self.focus_handle);
        selection_handlers(root, cx)
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
                    .gap(DynamicSpacing::Base06.rems(cx))
                    .px(DynamicSpacing::Base04.rems(cx))
                    .py(DynamicSpacing::Base02.rems(cx))
                    .border_b_1()
                    .border_color(cx.theme().colors().border_variant)
                    .children(
                        COLUMNS
                            .iter()
                            .map(|column| column_heading(column.heading, column.width)),
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
                        move |index, _window, cx| {
                            render_row(&rows, index, &render_base, &selection, &view, cx)
                        },
                    ))
                }
            })
    }
}

/// One cookie row: the nine attribute cells, each truncating with its full value in a
/// tooltip.
fn render_row(
    rows: &[ParsedCookie],
    index: usize,
    base: &ElementId,
    selection: &CellSelector,
    view: &WeakEntity<CookiesTab>,
    cx: &mut App,
) -> AnyElement {
    let Some(row) = rows.get(index) else {
        return div().into_any_element();
    };
    let row_id = row.key(base);
    let cells = row.cells();
    h_flex()
        .id(row_id.clone())
        .w_full()
        .gap(DynamicSpacing::Base06.rems(cx))
        .px(DynamicSpacing::Base04.rems(cx))
        .py(DynamicSpacing::Base04.rems(cx))
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .children(
            COLUMNS
                .iter()
                .zip(cells)
                .enumerate()
                .map(|(column_index, (column, text))| {
                    let cell_index = index * COLUMNS.len() + column_index;
                    column.show(
                        child_id(&row_id, column.id),
                        cell_index,
                        text,
                        selection,
                        view,
                        cx,
                    )
                }),
        )
        .into_any_element()
}

impl Column {
    /// This column's cell of one row: the text a user reads, or a flag pill, or — where
    /// the `Set-Cookie` header did not set the attribute — the column's width and
    /// nothing in it, so a blank never reads as a value and the columns keep lining up
    /// from row to row.
    fn show<V: SelectableCells>(
        &self,
        id: ElementId,
        index: usize,
        text: SharedString,
        selection: &CellSelector,
        view: &WeakEntity<V>,
        cx: &App,
    ) -> AnyElement {
        match self.kind {
            ColumnKind::Text if text.as_ref() != ABSENT => {
                cell(id, index, text, self.width, selection, view, cx).into_any_element()
            }
            ColumnKind::Flag => flag_cell(id, self.width, &text),
            ColumnKind::Text => blank_cell(self.width),
        }
    }
}

/// A boolean attribute as a flag pill: the icon and the word carry the meaning together,
/// never the icon or its colour alone (REQ-NFR-06). An attribute the header did not set
/// draws nothing.
fn flag_cell(id: ElementId, width: ColumnWidth, value: &str) -> AnyElement {
    let pill = match value {
        FLAG_SET => Some(
            Badge::new(FLAG_SET)
                .icon(IconName::Check)
                .color(Color::Success),
        ),
        FLAG_UNSET => Some(
            Badge::new(FLAG_UNSET)
                .icon(IconName::Minus)
                .color(Color::Muted),
        ),
        // The header did not set the attribute: no pill, only the blank below.
        _ => None,
    };
    let Some(pill) = pill else {
        return blank_cell(width);
    };
    width
        .sized(h_flex().items_center())
        .id(id.clone())
        .debug_selector(move || id.to_string())
        .child(pill)
        .into_any_element()
}

/// An absent attribute: the column's width and nothing in it — blank, so an unfilled
/// attribute reads as "the header did not set this" rather than as a value (#230). The
/// row's cells still hold [`ABSENT`], so what a copy takes says what the header did.
fn blank_cell(width: ColumnWidth) -> AnyElement {
    width.sized(div()).into_any_element()
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
        Some(true) => FLAG_SET.into(),
        Some(false) => FLAG_UNSET.into(),
        None => ABSENT.into(),
    }
}

#[cfg(test)]
#[path = "cookies_tab_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cookies_selection_tests.rs"]
mod selection_tests;

#[cfg(test)]
#[path = "cookies_columns_tests.rs"]
mod columns_tests;
