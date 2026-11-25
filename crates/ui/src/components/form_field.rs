//! A labelled form field: the caption above a field, the boxed field itself, and the
//! help text or inline error under it — one field's whole story in one place.
//!
//! `Decision:` this is the smallest thing that makes two dialogs look like one product
//! (the [`Modal`] frame's own rule): without it every form lays out its own
//! label/field/error trio, and "inline next to the field" means something different in
//! each. The field *widget* stays the caller's (a [`TextInput`], a [`TextArea`], a
//! future date picker) — this wraps whatever it is in the shared caption, box and
//! error slot. The box and caption come from the design pack's field anatomy
//! (`docs/postman_redesign_pack/01_specs/design_system.md` › Feedback: `InlineError` is
//! the error that lives with its field).

use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder,
};
use theme::ActiveTheme;

use crate::color::Color;
use crate::components::label::Label;
use crate::spacing::DynamicSpacing;
use crate::v_flex;

/// One labelled field: `label` over `content`, with `hint` or `error` under it.
#[derive(IntoElement)]
pub struct FormField {
    name: SharedString,
    label: SharedString,
    content: Option<AnyElement>,
    hint: Option<SharedString>,
    error: Option<SharedString>,
}

impl FormField {
    /// A field named `label`, identified by the stable `name` (its test selectors derive
    /// from it).
    pub fn new(name: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            content: None,
            hint: None,
            error: None,
        }
    }

    /// The field widget itself.
    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    /// Help text under the field: what it wants, when nothing has gone wrong.
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// The inline error under the field. It replaces the hint: the error is what the
    /// field needs saying now.
    pub fn error(mut self, error: impl Into<SharedString>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// The selector the caption of field `name` paints under (tests find it by this).
    pub fn label_selector(name: &str) -> String {
        format!("form-field-{name}-label")
    }

    /// The selector the hint of field `name` paints under.
    pub fn hint_selector(name: &str) -> String {
        format!("form-field-{name}-hint")
    }

    /// The selector the inline error of field `name` paints under.
    pub fn error_selector(name: &str) -> String {
        format!("form-field-{name}-error")
    }
}

impl RenderOnce for FormField {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let label_selector = Self::label_selector(&self.name);
        let hint_selector = Self::hint_selector(&self.name);
        let error_selector = Self::error_selector(&self.name);
        let error = self.error;
        let hint = self.hint;
        v_flex()
            .id(ElementId::from(self.name.to_string()))
            .w_full()
            .min_w_0()
            .gap(DynamicSpacing::Base02.rems(cx))
            // The caption is the field's accessible name: a group label around the
            // widget, so a screen reader announces "Name" before the field it is for.
            .role(gpui::Role::Group)
            .aria_label(self.label.clone())
            .child(
                div()
                    .debug_selector(move || label_selector)
                    .child(
                        Label::new(self.label)
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .px(DynamicSpacing::Base03.rems(cx))
                    .border_1()
                    .border_color(cx.theme().colors().border)
                    .rounded_sm()
                    .children(self.content),
            )
            .map(|field| match (error, hint) {
                (Some(error), _) => field.child(
                    div()
                        .debug_selector(move || error_selector)
                        .child(Label::new(error).size(LabelSize::Small).color(Color::Error)),
                ),
                (None, Some(hint)) => field.child(
                    div()
                        .debug_selector(move || hint_selector)
                        .child(Label::new(hint).size(LabelSize::Small).color(Color::Muted)),
                ),
                (None, None) => field,
            })
    }
}

#[cfg(test)]
#[path = "form_field_tests.rs"]
mod tests;
