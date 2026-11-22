//! The story: every component [`crate::prelude`] exports, in one view.
//!
//! This is the design system's proof of life — the test view `story_tests.rs` draws to
//! know that every component lays out in both themes, and the place a reviewer looks to
//! see what a component *is*. It ships with the tests, not with the app: the selectors
//! its sections record are GPUI test hooks (the same one `components/tab.rs` uses), which
//! only exist in test builds.
//!
//! One section per component, each root carrying the selector [`STORY_SECTIONS`] names —
//! that list is the single source of truth the view and the tests share.

use gpui::Entity;

use crate::prelude::*;

/// The debug selector of every story section, in the order the story draws them. Both
/// [`ComponentStory`] and `story_tests.rs` read this: the view to name its section roots,
/// the tests to prove each one laid out.
pub const STORY_SECTIONS: &[&str] = &[
    "story-button",
    "story-icon-button",
    "story-label",
    "story-icon",
    "story-tab-bar",
    "story-table",
    "story-toggle",
    "story-tooltip",
    "story-modal",
    "story-toast",
    "story-badge",
    "story-divider",
    "story-key-binding",
    "story-context-menu",
    "story-list",
    "story-empty-state",
];

/// How many rows the story's list section shows — a page, not a page *onto* thousands
/// (`components/list.rs` covers the virtualized case).
const STORY_LIST_ROWS: usize = 8;

/// The height of the story's list section, so the list is bounded like a real pane.
const STORY_LIST_HEIGHT_PX: f32 = 96.;

/// The height of the story's empty-state section — the block stands where a pane's
/// content would be, so it needs a pane of its own to center in.
const STORY_EMPTY_STATE_HEIGHT_PX: f32 = 120.;

/// One section: its root carries the selector the tests lay out under.
fn story_section(name: &'static str, content: AnyElement, cx: &App) -> AnyElement {
    v_flex()
        .id(name)
        .debug_selector(|| name.into())
        .gap(DynamicSpacing::Base03.rems(cx))
        .child(content)
        .into_any_element()
}

/// Every component of the design system, drawn once.
pub struct ComponentStory {
    button_focus_handle: FocusHandle,
    button_clicks: usize,
    tab_bar: Entity<TabBar>,
    context_menu: Entity<ContextMenu>,
}

impl ComponentStory {
    /// The story with every component mounted.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            button_focus_handle: cx.focus_handle(),
            button_clicks: 0,
            tab_bar: cx.new(|cx| {
                TabBar::new(cx).with_tabs(vec![
                    Tab::new("story-tab-params", "Params"),
                    Tab::new("story-tab-headers", "Headers"),
                    Tab::new("story-tab-body", "Body").disabled(true),
                ])
            }),
            context_menu: cx.new(|cx| {
                ContextMenu::new(cx)
                    .entry("Duplicate", |_, _, _| {})
                    .entry("Rename", |_, _, _| {})
                    .separator()
                    .disabled_entry("Delete")
            }),
        }
    }

    /// The focus handle of the story's primary button: how a test drives its keyboard.
    pub fn button_focus_handle(&self) -> FocusHandle {
        self.button_focus_handle.clone()
    }

    /// How many times the story's primary button has been activated.
    pub fn button_clicks(&self) -> usize {
        self.button_clicks
    }

    /// Every section, in [`STORY_SECTIONS`] order. The names bind positionally and the
    /// content is written beside its own name, so the two cannot drift apart silently:
    /// if the const and this list ever disagree on length, the story renders no sections
    /// and `story_tests::all_components_render_in_both_themes` fails on every name.
    fn sections(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let [
            button,
            icon_button,
            label,
            icon,
            tab_bar,
            table,
            toggle,
            tooltip,
            modal,
            toast,
            badge,
            divider,
            key_binding,
            context_menu,
            list,
            empty_state,
        ] = STORY_SECTIONS
        else {
            return Vec::new();
        };
        let sections: [(&'static str, AnyElement); 16] = [
            (*button, self.button_section(cx)),
            (*icon_button, self.icon_button_section(cx)),
            (*label, self.label_section(cx)),
            (*icon, self.icon_section(cx)),
            (*tab_bar, self.tab_bar.clone().into_any_element()),
            (*table, self.table_section()),
            (*toggle, self.toggle_section(cx)),
            (*tooltip, self.tooltip_section(cx)),
            (*modal, self.modal_section(cx)),
            (*toast, self.toast_section()),
            (*badge, self.badge_section(cx)),
            (*divider, Divider::horizontal().into_any_element()),
            (
                *key_binding,
                KeyBinding::for_action(&Activate).into_any_element(),
            ),
            (*context_menu, self.context_menu.clone().into_any_element()),
            (*list, self.list_section()),
            (*empty_state, self.empty_state_section()),
        ];
        sections
            .into_iter()
            .map(|(name, content)| story_section(name, content, cx))
            .collect()
    }

    fn button_section(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(
                Button::new("story-button-primary", "Send")
                    .style(ButtonStyle::Filled)
                    .size(ButtonSize::Large)
                    .focus_handle(self.button_focus_handle.clone())
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.button_clicks += 1;
                        cx.notify();
                    })),
            )
            .child(Button::new("story-button-outlined", "Cancel").style(ButtonStyle::Outlined))
            .child(Button::new("story-button-disabled", "Disabled").disabled(true))
            .into_any_element()
    }

    fn icon_button_section(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(IconButton::new(
                "story-icon-button-close",
                IconName::X,
                Tooltip::text("Close tab"),
            ))
            .child(
                IconButton::new(
                    "story-icon-button-send",
                    IconName::Send,
                    Tooltip::text("Send request"),
                )
                .loading(true),
            )
            .into_any_element()
    }

    fn label_section(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(Label::new("Content-Type"))
            .child(Label::new("A muted label").color(Color::Muted))
            .child(Headline::new("A headline").size(HeadlineSize::Small))
            .into_any_element()
    }

    fn icon_section(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(Icon::new(IconName::Send))
            .child(Icon::new(IconName::Search).size(IconSize::Large))
            .child(Icon::new(IconName::CircleCheck).color(Color::Success))
            .into_any_element()
    }

    fn table_section(&self) -> AnyElement {
        Table::new("story-table")
            .column(TableColumn::new("Key"))
            .column(TableColumn::new("Value"))
            .row(
                TableRow::new("story-table-row-accept")
                    .cells([Label::new("Accept"), Label::new("application/json")]),
            )
            .row(
                TableRow::new("story-table-row-agent")
                    .cells([Label::new("User-Agent"), Label::new("pustman/0.1")]),
            )
            .striped(true)
            .into_any_element()
    }

    fn toggle_section(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap(DynamicSpacing::Base08.rems(cx))
            .child(Toggle::new(
                "story-toggle-checkbox",
                "Enable",
                ToggleState::Selected,
            ))
            .child(
                Toggle::new(
                    "story-toggle-switch",
                    "Follow redirects",
                    ToggleState::Unselected,
                )
                .kind(ToggleKind::Switch),
            )
            .into_any_element()
    }

    fn tooltip_section(&self, cx: &mut Context<Self>) -> AnyElement {
        // A tooltip is data on its host, not a free-floating element — this is how every
        // control carries one (`components/tooltip.rs`), so the story shows the host.
        div()
            .id("story-tooltip-host")
            .px(DynamicSpacing::Base04.rems(cx))
            .py(DynamicSpacing::Base02.rems(cx))
            .rounded_sm()
            .border_1()
            .border_color(cx.theme().colors().border)
            .child(Label::new("Hover for a tooltip"))
            .tooltip(Tooltip::with_meta("Send request", "Sends the request in this tab").builder())
            .into_any_element()
    }

    fn modal_section(&self, cx: &mut Context<Self>) -> AnyElement {
        Modal::new("Import collection")
            .content(Label::new("Choose a Postman v2.1 collection to import."))
            .footer(
                h_flex()
                    .justify_end()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .child(Button::new("story-modal-cancel", "Cancel"))
                    .child(Button::new("story-modal-import", "Import").style(ButtonStyle::Filled)),
            )
            .into_any_element()
    }

    fn toast_section(&self) -> AnyElement {
        Toast::new("story-toast", "Request sent")
            .severity(Severity::Success)
            .action("Undo", |_, _, _| {})
            .into_any_element()
    }

    fn badge_section(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(Badge::new("200 OK").severity(Severity::Success))
            .child(Badge::new("GET").color(Color::Accent))
            .child(Badge::new("Saved").icon(IconName::Check))
            .into_any_element()
    }

    fn list_section(&self) -> AnyElement {
        div()
            .h(px_to_rems(STORY_LIST_HEIGHT_PX))
            .w_full()
            .child(VirtualList::new(
                "story-list",
                STORY_LIST_ROWS,
                |index| ElementId::from(format!("story-list-row-{index}")),
                |index, _window, _cx| Label::new(format!("Row {index}")).into_any_element(),
            ))
            .into_any_element()
    }

    fn empty_state_section(&self) -> AnyElement {
        // Its own id: the section root already carries the section name, and every id
        // in a frame is unique (`references/performance.md`).
        div()
            .h(px_to_rems(STORY_EMPTY_STATE_HEIGHT_PX))
            .w_full()
            .child(
                EmptyState::new("story-empty-state-block", IconName::Search, "No matches")
                    .description("Nothing matches the current search.")
                    .child(Button::new("story-empty-state-action", "Clear search")),
            )
            .into_any_element()
    }
}

impl Render for ComponentStory {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("component-story")
            .size_full()
            .font_ui(cx)
            .gap(DynamicSpacing::Base08.rems(cx))
            .p(DynamicSpacing::Base08.rems(cx))
            .bg(cx.theme().colors().background)
            .children(self.sections(cx))
    }
}

#[cfg(test)]
#[path = "story_tests.rs"]
mod tests;
