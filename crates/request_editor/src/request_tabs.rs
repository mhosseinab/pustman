//! The request tab strip's tabs (pack S03, mockup section 1): Params / Auth / Headers /
//! Body / Settings, with the enabled-row count on Params and Headers and a "set" dot on
//! Auth and Body. The badges are recomputed from the draft whenever it changes
//! ([`RequestEditor::refresh_tab_badges`]), never in `render`; the "Cookies" link at the
//! strip's right end opens the cookie jar through the existing `cookies::Manage`.

use api_model::{HttpRequest, KeyValue};
use gpui::{AnyElement, Context};
use pustman_actions::cookies::Manage;
use ui::prelude::*;

use crate::RequestEditor;

/// Test hook: the "Cookies" link at the right end of the request tab strip.
pub const COOKIES_LINK_SELECTOR: &str = "request-cookies-link";

/// What the tab strip shows beside each tab's label.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabBadges {
    /// Enabled query and path rows with a key; `None` when there are none.
    pub params: Option<usize>,
    /// Enabled header rows with a key; `None` when there are none.
    pub headers: Option<usize>,
    /// The request carries its own credentials (not inherit, not "no auth").
    pub auth_set: bool,
    /// The request carries a body.
    pub body_set: bool,
}

impl TabBadges {
    /// The badges `draft` earns.
    pub fn of(draft: &HttpRequest) -> Self {
        Self {
            params: nonzero(enabled_rows(&draft.params) + enabled_rows(&draft.path_variables)),
            headers: nonzero(enabled_rows(&draft.headers)),
            auth_set: !matches!(draft.auth, api_model::Auth::Inherit | api_model::Auth::None),
            body_set: !draft.body.is_none(),
        }
    }
}

fn enabled_rows(rows: &[KeyValue]) -> usize {
    rows.iter()
        .filter(|row| row.enabled && !row.key.is_empty())
        .count()
}

fn nonzero(count: usize) -> Option<usize> {
    (count > 0).then_some(count)
}

/// The tabs in strip order, stamped with `badges`. The ids are the test hooks every
/// existing test clicks by.
pub(crate) fn request_tabs(badges: TabBadges) -> Vec<Tab> {
    let set = || Some(SharedString::new_static("set"));
    vec![
        Tab::new(
            SharedString::new_static("request-tab-params"),
            SharedString::new_static("Params"),
        )
        .count(badges.params),
        Tab::new(
            SharedString::new_static("request-tab-auth"),
            SharedString::new_static("Auth"),
        )
        .indicator(badges.auth_set.then(set).flatten()),
        Tab::new(
            SharedString::new_static("request-tab-headers"),
            SharedString::new_static("Headers"),
        )
        .count(badges.headers),
        Tab::new(
            SharedString::new_static("request-tab-body"),
            SharedString::new_static("Body"),
        )
        .indicator(badges.body_set.then(set).flatten()),
        Tab::new(
            SharedString::new_static("request-tab-settings"),
            SharedString::new_static("Settings"),
        ),
    ]
}

impl RequestEditor {
    /// What the tab strip shows beside each label now.
    pub fn tab_badges(&self) -> TabBadges {
        self.tab_badges
    }

    /// Recompute the badges from the draft and re-stamp the strip when they moved.
    pub(super) fn refresh_tab_badges(&mut self, cx: &mut Context<Self>) {
        let badges = TabBadges::of(&self.draft);
        if badges == self.tab_badges {
            return;
        }
        self.tab_badges = badges;
        self.tab_bar
            .update(cx, |bar, cx| bar.set_tabs(request_tabs(badges), cx));
    }

    /// The tab strip with the "Cookies" link at its right end.
    pub(super) fn render_tab_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .w_full()
            .min_w_0()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(div().flex_1().min_w_0().child(self.tab_bar.clone()))
            .child(
                div().debug_selector(|| COOKIES_LINK_SELECTOR.to_string()).child(
                    Button::new("request-cookies-link", SharedString::new_static("Cookies"))
                        .style(ButtonStyle::Transparent)
                        .label_color(Color::Accent)
                        .tooltip(Tooltip::for_action("Open the cookie jar", &Manage))
                        .on_click(|_event, window, cx| {
                            window.dispatch_action(Box::new(Manage), cx)
                        }),
                ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "request_tabs_badge_tests.rs"]
mod badge_tests;
