//! The design system's empty state (Feedback › EmptyState,
//! `docs/postman_redesign_pack/01_specs/design_system.md`): one icon, a title, a line of
//! context and a call to action — the block every pane draws where its content would be.
//!
//! `Decision:` the sizes are fixed here, not caller-chosen, so no pane can fall back to
//! a caption below the empty-state scale again (#234): the icon draws at
//! [`IconSize::Large`] (the icon scale's "empty states and headlines" tier), the title
//! at [`HeadlineSize::Large`] — at or above the design system's 16 px empty-state
//! headline — and the copy at [`LabelSize::Large`] (16 px), the pack's smallest
//! "do not use tiny text" tier being 12/16.

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, Styled, Window, div,
};

use crate::color::Color;
use crate::components::label::{Headline, Label, LabelSize};
use crate::icons::{Icon, IconName, IconSize};
use crate::spacing::DynamicSpacing;
use crate::typography::HeadlineSize;
use crate::v_flex;

/// The empty state of one pane: icon, title, copy and whatever actions answer it.
///
/// Children sit under the copy — the call to action row and any hint rows. The whole
/// block fills its container and centers, so a caller wraps it where the pane's content
/// would have been.
///
/// Test hooks: `VisualTestContext::debug_bounds` finds the block under `id`, and its
/// icon, title and copy under `"{id}-icon"`, `"{id}-title"` and `"{id}-description"`
/// (the `toast.rs` selector pattern).
#[derive(IntoElement)]
pub struct EmptyState {
    id: ElementId,
    icon: IconName,
    title: SharedString,
    description: Option<SharedString>,
    children: Vec<AnyElement>,
}

impl EmptyState {
    /// An empty state with no copy and no actions yet.
    pub fn new(id: impl Into<ElementId>, icon: IconName, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            icon,
            title: title.into(),
            description: None,
            children: Vec::new(),
        }
    }

    /// The one line of context under the title.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl ParentElement for EmptyState {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = self.id.to_string();
        let root_selector = id.clone();
        let icon_selector = format!("{id}-icon");
        let title_selector = format!("{id}-title");
        let description_selector = format!("{id}-description");
        let description = self.description;
        v_flex()
            .id(self.id)
            .debug_selector(move || root_selector)
            .size_full()
            .items_center()
            .justify_center()
            .gap(DynamicSpacing::Base06.rems(cx))
            .child(
                div().debug_selector(move || icon_selector).child(
                    Icon::new(self.icon)
                        .size(IconSize::Large)
                        .color(Color::Muted),
                ),
            )
            // The copy centers and wraps in a narrow dock: an empty state must not
            // overflow the pane it is standing in for.
            .child(
                div()
                    .debug_selector(move || title_selector)
                    .max_w_full()
                    .text_center()
                    .child(
                        Headline::new(self.title)
                            .size(HeadlineSize::Large)
                            .color(Color::Default),
                    ),
            )
            .when_some(description, |this, description| {
                this.child(
                    div()
                        .debug_selector(move || description_selector)
                        .max_w_full()
                        .text_center()
                        .child(
                            Label::new(description)
                                .size(LabelSize::Large)
                                .color(Color::Muted),
                        ),
                )
            })
            .children(self.children)
    }
}
