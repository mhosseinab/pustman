//! cURL paste and Copy as cURL (PRD REQ-HTTP-13, S27c).
//!
//! Pasting a cURL command into the URL bar imports the request it describes, as one
//! undoable change; `request::CopyAsCurl` writes the draft back out as a cURL command.
//! The conversion itself is `api_model::curl`'s ([`parse_curl`], [`to_curl`]) — this
//! module is only the editor's side of it.
//!
//! `Decision:` the `text_input::Paste` capture sits on the element that wraps the URL
//! bar in [`crate::RequestEditor::render`], not on the editor root with a focus check:
//! the dispatch path reaches that wrapper exactly while focus is inside the URL bar and
//! never while a Params or Headers cell has focus (where a paste must stay a plain
//! paste) — the same scope in element form. `UrlBar`'s URL field and its focus handle
//! are private to `url_bar`, which this step's Edits do not cover, so no exact focus
//! check is reachable from here. The one field the wrapper also covers is the URL bar's
//! transient custom-method field: pasting a cURL command while a method token is being
//! typed imports too, which is what "paste a cURL command in the URL bar" means there
//! (a cURL command is not a method token and would be rejected).
//!
//! `Decision:` the undo is an inline "Undo import" row under the URL bar — a [`Button`],
//! so it is a tab stop Enter and Space activate — instead of a keystroke:
//! `text_input::Undo` (Ctrl/Cmd-Z) would have to be captured across the whole editor to
//! be reachable right after a paste, which would swallow the undo of a table-cell edit,
//! and a dedicated action would need one this step's Facts say not to add.

use api_model::{HttpRequest, parse_curl, to_curl};
use gpui::{ClipboardItem, Context, Window};
use pustman_actions::request;
use text_input::input::Paste;
use ui::prelude::*;

use crate::RequestEditor;
use crate::headers::Headers;
use crate::params::Params;
use crate::send::InlineError;
use crate::url_bar::UrlBar;

/// Test hook: the row under the URL bar that says a cURL command was imported and
/// offers to undo it.
pub const UNDO_IMPORT_SELECTOR: &str = "undo-curl-import";

impl RequestEditor {
    /// The [`Paste`] captured over the URL bar: a clipboard holding a cURL command is
    /// imported as the request it describes instead of being typed into the field.
    /// Anything else — and any paste elsewhere in the editor — keeps bubbling to the
    /// focused field's own paste.
    pub(super) fn handle_paste_curl(
        &mut self,
        _: &Paste,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let command = text.trim_start();
        if !command.starts_with("curl ") {
            return;
        }
        // The command is this editor's now: it never lands in the URL field, not even
        // when it fails to parse (the field keeps what it had).
        cx.stop_propagation();
        self.import_curl(command, window, cx);
    }

    /// `request::CopyAsCurl`: the draft as a cURL command on the clipboard.
    /// The command is never logged — it carries the request's headers and body verbatim.
    pub(super) fn handle_copy_as_curl(
        &mut self,
        _: &request::CopyAsCurl,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.write_to_clipboard(ClipboardItem::new_string(to_curl(&self.draft)));
    }

    /// The undo row under the URL bar, while a cURL import can be taken back.
    pub(super) fn render_import_undo(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        // No import to undo renders no row at all.
        self.pre_import_request.as_ref()?;
        Some(
            h_flex()
                .debug_selector(|| UNDO_IMPORT_SELECTOR.to_string())
                .gap(DynamicSpacing::Base03.rems(cx))
                .child(
                    Label::new(SharedString::new_static("Imported a cURL command"))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
                .child(
                    Button::new("undo-curl-import", SharedString::new_static("Undo import"))
                        .size(ButtonSize::Medium)
                        .style(ButtonStyle::Outlined)
                        .tooltip(Tooltip::text(SharedString::new_static(
                            "Restore the request as it was before the import",
                        )))
                        .on_click(
                            cx.listener(|this, _event, window, cx| this.undo_import(window, cx)),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Import `command` as this tab's request, keeping the request it replaces for one
    /// undo. A command that does not parse changes nothing and says why under the URL
    /// bar, where the paste happened (AGENTS.md — errors reach the user).
    fn import_curl(&mut self, command: &str, window: &mut Window, cx: &mut Context<Self>) {
        let request = match parse_curl(command) {
            Ok(request) => request,
            Err(error) => {
                self.inline_error = Some(InlineError {
                    message: format!("Could not import the cURL command: {error}"),
                    fix: String::from(
                        "Paste a whole cURL command — the text a browser's Network tab or a shell copies.",
                    ),
                });
                cx.notify();
                return;
            }
        };
        self.pre_import_request = Some(self.draft.clone());
        self.replace_request(request, window, cx);
    }

    /// Take the last import back in one step: the request it replaced is again the
    /// draft, the URL bar and both tables — and the tab's dirty dot is again whatever
    /// that request was before the import.
    fn undo_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(previous) = self.pre_import_request.take() else {
            return;
        };
        self.replace_request(previous, window, cx);
    }

    /// Make `request` this tab's request everywhere it shows: the draft, and the URL
    /// bar, Params and Headers rebuilt from it — the same construction
    /// [`crate::RequestEditor::new`] uses, because an import replaces the whole request
    /// rather than mapping it field by field.
    fn replace_request(
        &mut self,
        request: HttpRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.draft = request.clone();
        let url_bar =
            cx.new(|cx| UrlBar::new(request.method.clone(), request.url.clone(), window, cx));
        self._url_bar_subscription = cx.subscribe_in(&url_bar, window, Self::on_url_bar_event);
        self.url_bar = url_bar.clone();
        let params = cx.new(|cx| Params::new(&request, url_bar, window, cx));
        self._params_subscription = cx.subscribe_in(&params, window, Self::on_params_event);
        self.params = params;
        let headers = cx.new(|cx| Headers::new(&request, window, cx));
        self._headers_subscription = cx.subscribe_in(&headers, window, Self::on_headers_event);
        self.headers = headers;
        self.inline_error = None;
        cx.notify();
        // The paste happened in the URL field; the rebuilt bar is a new field to type
        // in, so the cursor goes where the user left it.
        self.focus_url_bar(window, cx);
    }
}

#[cfg(test)]
#[path = "curl_paste_tests.rs"]
mod tests;
