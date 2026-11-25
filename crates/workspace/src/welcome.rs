//! The [`Welcome`] item: the pane's empty state, and the tab `NewTab` opens when no crate
//! answers `NewRequest`. Real UI, not a placeholder (S48u3, mockup section 2):
//!
//! - **No workspace open** — "Start with a workspace": what a workspace is, New
//!   workspace… (primary), Open workspace…, Import from Postman… (disabled until there is
//!   a workspace to import into, with the reason in its tooltip) and the Recent list —
//!   one click opens a recorded folder in this window.
//! - **A workspace open** — its name, New Request and New collection… (the Collections
//!   panel's own "＋ New", run in the panel), and the same Recent list minus the open one.
//!
//! New Request and the sidebar shortcut stay in both states (REQ-UI-02).
//!
//! `Decision:` the welcome finds its window's workspace through `window.root()` while
//! drawing instead of holding a handle: every Welcome — the pane's own and each one
//! `NewTab` opens — is built by the pane, which has no workspace handle to give it.
//! Reading the window's [`WorkspaceOpen`] while drawing is what makes it redraw when a
//! workspace opens ([`WorkspaceOpen`] notifies on every open).

use std::path::Path;

use gpui::{
    Action, App, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Role,
    SharedString, StatefulInteractiveElement, Subscription, Window,
};
use pustman_actions::import_export::Import;
use pustman_actions::workspace::{CreateWorkspace, NewTab, OpenWorkspace, ToggleSidebar};
use ui::prelude::*;

use crate::Workspace;
use crate::item::Item;
use crate::recent_workspaces::{
    RecentEntry, RecentWorkspaces, recent_workspaces, recent_workspaces_failed,
};
use crate::workspace_open::WorkspaceOpen;

#[cfg(test)]
#[path = "welcome_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "welcome_states_tests.rs"]
mod states_tests;

/// The welcome view's stable id and test selector.
const WELCOME_ID: &str = "welcome";

/// The New Request button's stable id and test selector.
const NEW_REQUEST_ID: &str = "welcome-new-request";

/// The other calls to action's ids and test selectors.
pub(crate) const NEW_WORKSPACE_ID: &str = "welcome-new-workspace";
pub(crate) const OPEN_WORKSPACE_ID: &str = "welcome-open-workspace";
pub(crate) const IMPORT_ID: &str = "welcome-import";
pub(crate) const NEW_COLLECTION_ID: &str = "welcome-new-collection";

/// The key hints' test selectors: New Request's and the sidebar shortcut's keys.
pub(crate) const NEW_REQUEST_KEYS_SELECTOR: &str = "welcome-new-request-keys";
pub(crate) const TOGGLE_SIDEBAR_KEYS_SELECTOR: &str = "welcome-toggle-sidebar-keys";

/// The panel whose "＋ New" the open-workspace state offers as New collection….
const COLLECTIONS_PANEL_ID: &str = "collections";

/// What a Recent click says when this window cannot open a workspace.
const NO_OPENER_MESSAGE: &str = "This window cannot open a workspace — use Open workspace… instead";

/// How many recent folders the welcome lists; the rest are a `workspace::OpenRecent` away.
const MAX_RECENT_ROWS: usize = 5;

/// What the Recent list shows when the store could not be read.
const RECENT_FAILURE_MESSAGE: &str = "The recent workspaces could not be read.";

/// The failed-read line's test selector.
pub(crate) const RECENT_FAILURE_SELECTOR: &str = "welcome-recent-failure";

/// One Recent row's element id and test selector: the folder's path, its stable identity.
pub(crate) fn recent_row_id(folder: &Path) -> String {
    format!("welcome-recent-{}", folder.to_string_lossy())
}

/// The pane's empty state (see the module docs). Its Recent list is the app's one recent
/// list ([`recent_workspaces`]), redrawn whenever that list changes.
pub struct Welcome {
    _subscriptions: Vec<Subscription>,
}

impl Welcome {
    /// A fresh welcome item, following the app's recent list.
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Idempotent: the list is wired once per app, by whichever reader comes first.
        crate::recent_workspaces::init(cx);
        Self {
            _subscriptions: vec![cx.observe_global::<RecentWorkspaces>(|_welcome, cx| cx.notify())],
        }
    }

    /// The "Start with a workspace" state.
    fn no_workspace(&self, cx: &App) -> impl IntoElement {
        v_flex()
            .gap(DynamicSpacing::Base08.rems(cx))
            .child(
                h_flex()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .child(new_workspace_button(ButtonStyle::Filled))
                    .child(action_button(
                        OPEN_WORKSPACE_ID,
                        "Open workspace…",
                        None,
                        ButtonStyle::Outlined,
                        &OpenWorkspace,
                    ))
                    .child(
                        div().debug_selector(|| IMPORT_ID.to_owned()).child(
                            Button::new(IMPORT_ID, "Import from Postman…")
                                .size(ButtonSize::Large)
                                .style(ButtonStyle::Outlined)
                                .disabled(true)
                                .tooltip(Tooltip::text(
                                    "Create or open a workspace first — an import lands in it",
                                )),
                        ),
                    ),
            )
            .child(new_request_row(cx))
    }

    /// The state with a workspace open in this window.
    fn workspace_open(&self, offers_new_collection: bool, cx: &App) -> impl IntoElement {
        v_flex().gap(DynamicSpacing::Base08.rems(cx)).child(
            h_flex()
                .gap(DynamicSpacing::Base04.rems(cx))
                .child(new_request_row(cx))
                .when(offers_new_collection, |row| {
                    row.child(
                        div().debug_selector(|| NEW_COLLECTION_ID.to_owned()).child(
                            Button::new(NEW_COLLECTION_ID, "New collection…")
                                .size(ButtonSize::Large)
                                .style(ButtonStyle::Outlined)
                                .start_icon(Icon::new(IconName::Folder))
                                .on_click(|_, window, cx| {
                                    let Some(workspace) = window.root::<Workspace>().flatten()
                                    else {
                                        return;
                                    };
                                    workspace.update(cx, |workspace, cx| {
                                        workspace.run_panel_create(
                                            COLLECTIONS_PANEL_ID,
                                            window,
                                            cx,
                                        );
                                    });
                                }),
                        ),
                    )
                })
                .child(action_button(
                    IMPORT_ID,
                    "Import from Postman…",
                    None,
                    ButtonStyle::Outlined,
                    &Import,
                )),
        )
    }

    /// The Recent list: name and path per row, a click opens the folder in this window.
    /// The folder already open here is left out.
    fn recent_list(
        &self,
        opener: Option<&Entity<WorkspaceOpen>>,
        open_root: Option<&Path>,
        cx: &App,
    ) -> Option<AnyElement> {
        if recent_workspaces_failed(cx) {
            return Some(
                div()
                    .debug_selector(|| RECENT_FAILURE_SELECTOR.to_owned())
                    .child(Label::new(RECENT_FAILURE_MESSAGE).color(Color::Error))
                    .into_any_element(),
            );
        }
        let entries: Vec<&RecentEntry> = recent_workspaces(cx)
            .iter()
            .filter(|entry| Some(entry.path.as_path()) != open_root)
            .take(MAX_RECENT_ROWS)
            .collect();
        if entries.is_empty() {
            return None;
        }
        let opener = opener.cloned();
        let colors = cx.theme().colors();
        Some(
            v_flex()
                .w_full()
                .gap(DynamicSpacing::Base03.rems(cx))
                .child(eyebrow("Recent", cx))
                .child(
                    v_flex()
                        .id("welcome-recent")
                        .role(Role::List)
                        .w_full()
                        .border_1()
                        .rounded_md()
                        .border_color(colors.border_variant)
                        .bg(colors.surface_background)
                        .children(entries.into_iter().enumerate().map(|(index, entry)| {
                            recent_row(entry, index > 0, opener.clone(), cx)
                        })),
                )
                .into_any_element(),
        )
    }
}

impl Item for Welcome {
    fn tab_title(&self, _cx: &App) -> SharedString {
        SharedString::new_static("Welcome")
    }
}

impl Render for Welcome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = window.root::<Workspace>().flatten();
        let opener = workspace
            .as_ref()
            .and_then(|workspace| WorkspaceOpen::for_window(workspace.entity_id(), cx));
        let open_root = opener
            .as_ref()
            .and_then(|opener| opener.read(cx).root().map(Path::to_path_buf));
        // The name the workspace file gives itself, falling back to the folder's.
        let opened_name = opener.as_ref().and_then(|opener| {
            opener.read(cx).snapshot().and_then(|snapshot| {
                snapshot.items.values().find_map(|item| match &item.body {
                    workspace_store::ItemBody::Workspace(document) => Some(document.name.clone()),
                    _ => None,
                })
            })
        });
        let offers_new_collection = workspace.as_ref().is_some_and(|workspace| {
            workspace
                .read(cx)
                .panel_create_action(COLLECTIONS_PANEL_ID, cx)
                .is_some()
        });

        let (title, description): (SharedString, SharedString) = match &open_root {
            Some(root) => (
                opened_name
                    .clone()
                    .unwrap_or_else(|| workspace_name(root))
                    .into(),
                SharedString::new_static(
                    "Its collections and environments are in the sidebar. Start a request, \
                     or add a collection to keep requests together.",
                ),
            ),
            None => (
                SharedString::new_static("Start with a workspace"),
                SharedString::new_static(
                    "A workspace is a folder on your disk. It holds your collections and \
                     environments as plain files, so you can share it with Git.",
                ),
            ),
        };
        let actions = match &open_root {
            Some(_) => self
                .workspace_open(offers_new_collection, cx)
                .into_any_element(),
            None => self.no_workspace(cx).into_any_element(),
        };
        let recent = self.recent_list(opener.as_ref(), open_root.as_deref(), cx);

        v_flex()
            .id(WELCOME_ID)
            .debug_selector(|| WELCOME_ID.to_owned())
            .size_full()
            .overflow_y_scroll()
            .font_ui(cx)
            .px(DynamicSpacing::Base32.rems(cx))
            .py(DynamicSpacing::Base24.rems(cx))
            .child(
                v_flex()
                    .max_w(px_to_rems(640.))
                    .gap(DynamicSpacing::Base06.rems(cx))
                    .child(
                        h_flex()
                            .gap(DynamicSpacing::Base03.rems(cx))
                            .child(
                                div().debug_selector(|| format!("{WELCOME_ID}-icon")).child(
                                    Icon::new(IconName::Send)
                                        .size(IconSize::Medium)
                                        .color(Color::Accent),
                                ),
                            )
                            .child(eyebrow("Welcome to Pustman", cx)),
                    )
                    .child(
                        div()
                            .debug_selector(|| format!("{WELCOME_ID}-title"))
                            .min_w_0()
                            .child(Headline::new(title).size(HeadlineSize::Large)),
                    )
                    .child(
                        div()
                            .debug_selector(|| format!("{WELCOME_ID}-description"))
                            .child(
                                Label::new(description)
                                    .size(LabelSize::Large)
                                    .color(Color::Muted),
                            ),
                    )
                    .child(actions)
                    .child(shortcut_hint(
                        "Toggle Sidebar",
                        &ToggleSidebar,
                        TOGGLE_SIDEBAR_KEYS_SELECTOR,
                        cx,
                    ))
                    .children(recent),
            )
    }
}

/// The small upper-case section label the mockup's "Welcome to Pustman" and "Recent" use.
fn eyebrow(text: &'static str, cx: &App) -> impl IntoElement {
    div()
        .text_size(TextSize::XSmall.rems(cx))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(cx.theme().colors().text_muted)
        .child(SharedString::new_static(text).to_uppercase())
}

/// New workspace…: the primary call to action of the no-workspace state.
fn new_workspace_button(style: ButtonStyle) -> impl IntoElement {
    action_button(
        NEW_WORKSPACE_ID,
        "New workspace…",
        Some(IconName::Plus),
        style,
        &CreateWorkspace,
    )
}

/// A button that dispatches `action` from wherever the keyboard is — the app-level
/// handlers (`workspace_open`'s, `import`'s) answer it on any dispatch path.
fn action_button(
    id: &'static str,
    label: &'static str,
    icon: Option<IconName>,
    style: ButtonStyle,
    action: &dyn Action,
) -> impl IntoElement {
    let action = action.boxed_clone();
    div().debug_selector(move || id.to_owned()).child(
        Button::new(id, label)
            .size(ButtonSize::Large)
            .style(style)
            .map(|button| match icon {
                Some(icon) => button.start_icon(Icon::new(icon)),
                None => button,
            })
            .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx)),
    )
}

/// New Request with its shortcut beside it — not inside it: a key chip on the filled
/// accent background is unreadable.
fn new_request_row(cx: &App) -> impl IntoElement {
    h_flex()
        .gap(DynamicSpacing::Base04.rems(cx))
        .child(
            div().debug_selector(|| NEW_REQUEST_ID.to_owned()).child(
                Button::new(NEW_REQUEST_ID, "New Request")
                    .size(ButtonSize::Large)
                    .style(ButtonStyle::Outlined)
                    .start_icon(Icon::new(IconName::Plus))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(NewTab), cx)),
            ),
        )
        .child(
            div()
                .debug_selector(|| NEW_REQUEST_KEYS_SELECTOR.to_owned())
                .child(KeyBinding::from_keymap(&NewTab).size(TextSize::Default)),
        )
}

/// One Recent row: the workspace's name, its path muted at the right. A
/// keyboard-reachable list item; activating it opens the folder in this window.
fn recent_row(
    entry: &RecentEntry,
    divided: bool,
    opener: Option<Entity<WorkspaceOpen>>,
    cx: &App,
) -> impl IntoElement {
    let folder = entry.path.clone();
    let id = recent_row_id(&folder);
    let selector = id.clone();
    let name = entry.name.clone();
    let path: SharedString = folder.to_string_lossy().into_owned().into();
    let colors = cx.theme().colors();
    h_flex()
        .id(SharedString::from(id))
        .debug_selector(move || selector)
        .role(Role::ListItem)
        .aria_label(SharedString::from(format!("Open {name} — {path}")))
        .focusable()
        .tab_stop(true)
        .cursor_pointer()
        .w_full()
        .min_w_0()
        .gap(DynamicSpacing::Base06.rems(cx))
        .px(DynamicSpacing::Base06.rems(cx))
        .py(DynamicSpacing::Base04.rems(cx))
        .when(divided, |row| {
            row.border_t_1().border_color(colors.border_variant)
        })
        .hover(|row| row.bg(colors.ghost_element_hover))
        .focus_visible(|row| row.border_1().border_color(colors.border_focused))
        .tooltip(Tooltip::text(path.clone()).builder())
        .child(div().flex_none().child(Label::new(name)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .font_buffer(cx)
                .text_size(TextSize::Small.rems(cx))
                .text_color(colors.text_muted)
                .text_right()
                .truncate()
                .child(path),
        )
        .on_click(move |_, window, cx| {
            let Some(opener) = opener.as_ref() else {
                // Errors reach the user: a click that cannot open anything says so.
                if let Some(workspace) = window.root::<Workspace>().flatten() {
                    workspace.update(cx, |workspace, cx| {
                        workspace.show_toast(NO_OPENER_MESSAGE, Severity::Error, None, cx);
                    });
                }
                return;
            };
            let folder = folder.clone();
            opener.update(cx, |opener, cx| opener.open_folder(folder, cx));
        })
}

/// A workspace folder's display name: its last path component.
fn workspace_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned())
}

/// One hint row: what the shortcut does, and the keys that run it — read from the active
/// keymap at render time, so a rebound key cannot go stale here, and from the whole
/// keymap, so the hint stays while an application menu has the keyboard (#332).
fn shortcut_hint(
    label: &'static str,
    action: &dyn Action,
    keys_selector: &'static str,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .gap(DynamicSpacing::Base04.rems(cx))
        .child(
            Label::new(label)
                .size(LabelSize::Default)
                .color(Color::Muted),
        )
        .child(
            div()
                .debug_selector(move || keys_selector.to_owned())
                .child(KeyBinding::from_keymap(action).size(TextSize::Default)),
        )
}

#[cfg(test)]
#[path = "welcome_hints_tests.rs"]
mod hints_tests;
