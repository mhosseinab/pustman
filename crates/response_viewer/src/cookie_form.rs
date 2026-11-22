//! The Manage Cookies add/edit form (PRD REQ-HTTP-08): name, value, domain, path,
//! expiry and the Secure / HttpOnly flags, validated before a change reaches the jar.
//! The form collects and checks; [`crate::cookie_dialog`] owns the jar, the save and the
//! row list, and this file never touches either — the form speaks through
//! [`CookieFormEvent`] alone.
//!
//! Validation here covers only what [`CookieJar`](http_engine::CookieJar) cannot see:
//! that the required fields (name, domain) are filled and that the expiry text parses.
//! The jar validates the rest (the name is a token, the value is header-safe, the
//! domain is a scoping host, the expiry is in the future) and its `CookieError`'s
//! [`Display`](std::fmt::Display) — which names the cookie and never its value (PRD §7)
//! — is shown inline through [`CookieForm::show_error`]. There is no second cookie
//! validator here.
//!
//! `Decision:` expiry text is RFC 2822 — the exact format the dialog's Expires column
//! and `cookies_tab` display (`time`'s `Rfc2822`, already in the workspace), so a shown
//! expiry can be copied back into the form unchanged. The placeholder shows an example.
//! Empty means a session cookie (RFC 6265).
//!
//! `Decision:` the form is an inline panel inside the dialog, replacing the row list
//! while it is open: Escape cancels the form and not the dialog (the `find::FindBar`
//! key-listener pattern), Enter in a field and the Save button both run
//! [`CookieForm::submit`], and the list comes back — refreshed from the jar — after a
//! save.
//!
//! Cookie values are secrets (PRD §7): nothing here logs or Debug-prints one, and the
//! events carry [`CookieEntry`], whose `Debug` redacts the value.

use gpui::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Render, Subscription,
    Window,
};
use http_engine::CookieEntry;
use text_input::{TextInput, TextInputEvent};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc2822;
use ui::prelude::*;

use crate::child_id;

#[cfg(test)]
#[path = "cookie_form_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cookie_form_keys_tests.rs"]
mod keys_tests;

/// The width of a field's visible label column, so the fields line up.
const FIELD_LABEL_WIDTH_PX: f32 = 96.;

/// Where the form says which field is missing (AGENTS.md: errors are actionable).
const NAME_REQUIRED_MESSAGE: &str = "Name is required — a cookie is identified by its name.";
const DOMAIN_REQUIRED_MESSAGE: &str = "Domain is required — the host the cookie is scoped to.";

/// What the form says where the expiry text does not parse: the format, with the same
/// example the placeholder shows.
const EXPIRY_FORMAT_MESSAGE: &str = "Expiry must be an RFC 2822 timestamp, like Thu, 01 Jan 2026 12:00:00 +0000 — empty for a session cookie.";

/// Which field [`CookieForm::set_field`] fills. The fields are reachable by name, never
/// by tab index: a reorder must not change what a call means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CookieField {
    Name,
    Value,
    Domain,
    Path,
    Expires,
}

/// The row a form saves over: what [`CookieJar::edit`](http_engine::CookieJar::edit)
/// identifies a cookie by. No value lives here — the identity is not a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CookieIdentity {
    pub domain: String,
    pub path: String,
    pub name: String,
}

/// What the form tells its host ([`crate::cookie_dialog`]).
#[derive(Clone, Debug)]
pub enum CookieFormEvent {
    /// Save `entry`: replace the cookie `original` names, or add a new one when it is
    /// `None`.
    Save {
        /// The row the form edits over; `None` for a new cookie.
        original: Option<CookieIdentity>,
        /// What to store. Its `Debug` redacts the value (PRD §7).
        entry: CookieEntry,
    },
    /// Escape or Cancel: the form goes away and the jar is untouched.
    Cancelled,
}

/// The add/edit form (REQ-HTTP-08): one [`TextInput`] per cookie attribute, the two
/// flag toggles, the inline error and Save / Cancel.
pub struct CookieForm {
    /// The row the form edits over; `None` while adding a new cookie.
    original: Option<CookieIdentity>,
    /// What the headline says: "Add cookie" or "Edit cookie <name>", precomputed so
    /// `render` stays a pure read.
    title: SharedString,
    name: Entity<TextInput>,
    value: Entity<TextInput>,
    domain: Entity<TextInput>,
    path: Entity<TextInput>,
    expires: Entity<TextInput>,
    /// The Secure flag: HTTPS only.
    secure: bool,
    /// The HttpOnly flag: withheld from non-HTTP APIs.
    http_only: bool,
    /// Why the last save did not happen, shown under the fields — inline, where the
    /// change was typed (AGENTS.md: errors reach the user).
    error: Option<SharedString>,
    /// Kept so Enter in a field keeps reaching us; dropping it would silently stop
    /// saving (AGENTS.md › GPUI).
    _field_subscriptions: Vec<Subscription>,
}

impl CookieForm {
    /// A form for a cookie that does not exist yet.
    pub fn add(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(None, None, window, cx)
    }

    /// A form over `entry` as the jar holds it, prefilled — what a row's Edit control
    /// opens. The identity it saves over is the row's, captured here so moving the
    /// cookie (a rename) still replaces the right one.
    pub fn edit(entry: CookieEntry, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let identity = CookieIdentity {
            domain: entry.domain.clone(),
            path: entry.path.clone(),
            name: entry.name.clone(),
        };
        Self::build(Some(identity), Some(entry), window, cx)
    }

    /// Fill one field as keystrokes would (through [`TextInput::set_text`], so the same
    /// [`TextInputEvent::Changed`] fires). This is the form's public input API.
    pub fn set_field(&mut self, field: CookieField, text: &str, cx: &mut Context<Self>) {
        let input = match field {
            CookieField::Name => &self.name,
            CookieField::Value => &self.value,
            CookieField::Domain => &self.domain,
            CookieField::Path => &self.path,
            CookieField::Expires => &self.expires,
        };
        input.update(cx, |input, cx| input.set_text(text, cx));
    }

    /// Set the Secure flag.
    pub fn set_secure(&mut self, secure: bool, cx: &mut Context<Self>) {
        self.secure = secure;
        cx.notify();
    }

    /// Set the HttpOnly flag.
    pub fn set_http_only(&mut self, http_only: bool, cx: &mut Context<Self>) {
        self.http_only = http_only;
        cx.notify();
    }

    /// The inline error under the fields, if the last save did not happen.
    pub fn error(&self) -> Option<SharedString> {
        self.error.clone()
    }

    /// Show `message` under the fields — the dialog hands a refused [`CookieError`]'s
    /// (http_engine) Display here, and it never carries a cookie value (PRD §7).
    pub fn show_error(&mut self, message: SharedString, cx: &mut Context<Self>) {
        self.error = Some(message);
        cx.notify();
    }

    /// Validate the fields and emit [`CookieFormEvent::Save`] — what the Save button and
    /// Enter both run. An invalid form emits nothing and says why inline.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let entry = match self.build_entry(cx) {
            Ok(entry) => entry,
            Err(message) => {
                self.error = Some(message);
                cx.notify();
                return;
            }
        };
        self.error = None;
        cx.emit(CookieFormEvent::Save {
            original: self.original.clone(),
            entry,
        });
    }

    /// A form over `entry` (or empty, for a new cookie), its fields reporting through
    /// [`Self::on_field_event`].
    fn build(
        original: Option<CookieIdentity>,
        entry: Option<CookieEntry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (name, value, domain, path, expires, secure, http_only) = match &entry {
            Some(entry) => (
                entry.name.clone(),
                entry.value.clone(),
                entry.domain.clone(),
                entry.path.clone(),
                // An expiry RFC 2822 cannot express (a year past 9999) comes back as an
                // empty field: the form prefills only what it can round-trip.
                entry.expires.map_or_else(String::new, |when| {
                    when.format(&Rfc2822).unwrap_or_default()
                }),
                entry.secure,
                entry.http_only,
            ),
            None => (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                false,
                false,
            ),
        };
        let name = Self::make_field("e.g. sid", &name, window, cx);
        let value = Self::make_field("e.g. session-1234", &value, window, cx);
        let domain = Self::make_field("e.g. api.example.test", &domain, window, cx);
        let path = Self::make_field("e.g. / (empty = /)", &path, window, cx);
        let expires = Self::make_field(
            "Thu, 01 Jan 2026 12:00:00 +0000 (empty = session)",
            &expires,
            window,
            cx,
        );
        let _field_subscriptions = [&name, &value, &domain, &path, &expires]
            .into_iter()
            .map(|field| cx.subscribe(field, Self::on_field_event))
            .collect();
        let title = match &original {
            Some(identity) => SharedString::from(format!("Edit cookie {}", identity.name)),
            None => SharedString::new_static("Add cookie"),
        };
        Self {
            original,
            title,
            name,
            value,
            domain,
            path,
            expires,
            secure,
            http_only,
            error: None,
            _field_subscriptions,
        }
    }

    /// One empty-or-prefilled field, in the tab order (`request_editor::key_value_table`
    /// does the same for its cells).
    fn make_field(
        placeholder: &'static str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextInput> {
        let field = cx.new(|cx| {
            TextInput::new(window, cx).with_placeholder(SharedString::new_static(placeholder))
        });
        // `text_input`'s handle defaults to not being a tab stop; every field belongs in
        // the tab order (keyboard-first).
        field.read(cx).focus_handle(cx).tab_stop(true);
        if !text.is_empty() {
            field.update(cx, |field, cx| field.set_text(text, cx));
        }
        field
    }

    /// The [`CookieEntry`] the fields describe — or why there is not one. Only the
    /// checks the jar cannot make (REQ-HTTP-08): everything else is the jar's
    /// [`CookieError`](http_engine::CookieError), shown inline by the dialog.
    fn build_entry(&self, cx: &App) -> Result<CookieEntry, SharedString> {
        let name = self.name.read(cx).text().trim().to_owned();
        if name.is_empty() {
            return Err(SharedString::new_static(NAME_REQUIRED_MESSAGE));
        }
        let domain = self.domain.read(cx).text().trim().to_owned();
        if domain.is_empty() {
            return Err(SharedString::new_static(DOMAIN_REQUIRED_MESSAGE));
        }
        let expires_text = self.expires.read(cx).text().trim().to_owned();
        let expires = if expires_text.is_empty() {
            // An empty expiry is a session cookie (RFC 6265).
            None
        } else {
            Some(
                OffsetDateTime::parse(&expires_text, &Rfc2822)
                    .map_err(|_| SharedString::new_static(EXPIRY_FORMAT_MESSAGE))?,
            )
        };
        Ok(CookieEntry {
            domain,
            path: self.path.read(cx).text().trim().to_owned(),
            name,
            // The value is taken as typed: it is the one field whose spaces are
            // meaningful (the jar refuses `;` and control bytes, not spaces).
            value: self.value.read(cx).text(),
            expires,
            secure: self.secure,
            http_only: self.http_only,
        })
    }

    /// Enter in any field saves. [`TextInputEvent::Changed`] is ignored: the form reads
    /// its fields when it saves (its own setters emit `Changed` too).
    fn on_field_event(
        &mut self,
        _field: Entity<TextInput>,
        event: &TextInputEvent,
        cx: &mut Context<Self>,
    ) {
        if let TextInputEvent::Submitted = event {
            self.submit(cx);
        }
    }

    /// The form's own key: Escape cancels the form and stops there, so the dialog's
    /// Escape (close the dialog) does not also answer — the `find::FindBar` pattern.
    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.stop_propagation();
            cx.emit(CookieFormEvent::Cancelled);
        }
    }

    /// Escape's and Cancel's shared behavior: the form goes away, the jar untouched.
    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(CookieFormEvent::Cancelled);
    }

    /// One labeled field row: the label, then the field in a bordered box (`find`'s
    /// query box, same shape).
    fn render_field(&self, label: &'static str, field: &Entity<TextInput>, cx: &App) -> AnyElement {
        h_flex()
            .w_full()
            .items_center()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(
                div()
                    .flex_none()
                    .w(px_to_rems(FIELD_LABEL_WIDTH_PX))
                    .child(Label::new(label).size(LabelSize::Small).color(Color::Muted)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px(DynamicSpacing::Base03.rems(cx))
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .rounded_sm()
                    .child(field.clone()),
            )
            .into_any_element()
    }

    /// The Secure and HttpOnly checkboxes. The label is each control's accessible name
    /// (`Toggle`, the `find::FindBar` shape).
    fn render_flags(&self, base: &ElementId, cx: &mut Context<Self>) -> AnyElement {
        let secure_weak = cx.weak_entity();
        let http_only_weak = cx.weak_entity();
        h_flex()
            .w_full()
            .gap(DynamicSpacing::Base06.rems(cx))
            .child(
                Toggle::new(
                    child_id(base, "secure"),
                    SharedString::new_static("Secure — sent over HTTPS only"),
                    ToggleState::from(self.secure),
                )
                .on_change(move |state, _window, cx| {
                    if secure_weak
                        .update(cx, |form, cx| form.set_secure(state.is_selected(), cx))
                        .is_err()
                    {
                        // Handled: the form went away with its window while the toggle
                        // was up; there is nothing left to flag.
                    }
                }),
            )
            .child(
                Toggle::new(
                    child_id(base, "http-only"),
                    SharedString::new_static("HttpOnly — withheld from non-HTTP APIs"),
                    ToggleState::from(self.http_only),
                )
                .on_change(move |state, _window, cx| {
                    if http_only_weak
                        .update(cx, |form, cx| form.set_http_only(state.is_selected(), cx))
                        .is_err()
                    {
                        // Handled: the form went away with its window while the toggle
                        // was up; there is nothing left to flag.
                    }
                }),
            )
            .into_any_element()
    }

    /// Save and Cancel. Save runs the very [`Self::submit`] Enter runs.
    fn render_buttons(&self, base: &ElementId, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .w_full()
            .justify_end()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(
                Button::new(child_id(base, "save"), "Save")
                    .on_click(cx.listener(|this, _event, _window, cx| this.submit(cx))),
            )
            .child(
                Button::new(child_id(base, "cancel"), "Cancel")
                    .on_click(cx.listener(|this, _event, _window, cx| this.cancel(cx))),
            )
            .into_any_element()
    }
}

impl Render for CookieForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One base id per entity, so two forms in one frame never share element ids.
        let base = ElementId::from(("cookie-form", cx.entity_id()));
        let error = self.error.clone();
        v_flex()
            .id(child_id(&base, "form"))
            .w_full()
            .min_w_0()
            .gap(DynamicSpacing::Base04.rems(cx))
            .on_key_down(cx.listener(Self::on_key_down))
            .child(Headline::new(self.title.clone()).size(HeadlineSize::Small))
            .child(self.render_field("Name", &self.name, cx))
            .child(self.render_field("Value", &self.value, cx))
            .child(self.render_field("Domain", &self.domain, cx))
            .child(self.render_field("Path", &self.path, cx))
            .child(self.render_field("Expires", &self.expires, cx))
            .child(self.render_flags(&base, cx))
            .map(|this| match error {
                Some(message) => this.child(
                    Label::new(message)
                        .size(LabelSize::Small)
                        .color(Color::Error),
                ),
                None => this,
            })
            .child(self.render_buttons(&base, cx))
    }
}

impl Focusable for CookieForm {
    /// The form is one tab stop: it delegates focus to its first field, so typing works
    /// the moment the form opens (the `find::FindBar` contract).
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.name.read(cx).focus_handle(cx)
    }
}

impl EventEmitter<CookieFormEvent> for CookieForm {}
