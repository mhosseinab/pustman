//! The New Workspace form (S48u1): what `CreateWorkspace` opens. The user names the
//! workspace and picks the folder it goes in; the form shows the folder it will create
//! (`<location>/<slug of name>`), checks everything inline before Create is enabled, and
//! hands the create to [`WorkspaceOpen::create_named`] — the one create path, Git note
//! (REQ-COLLAB-06) included.
//!
//! `Decision:` the location check (exists, is a folder, is writable, already holds a
//! workspace) runs on a background task each time the name or location changes
//! (AGENTS.md › no blocking IO on the foreground thread). Create stays disabled until the
//! check for the current text has landed, so a stale answer can never enable it.
//!
//! `Decision:` "writable" is the folder's permission bits as `std::fs::Permissions`
//! reports them — nothing is written before Create (the form's promise). A folder the
//! bits call writable that still refuses the write fails Create, and that reason shows
//! inline in the form.
//!
//! `Decision:` the default location is the last parent folder a workspace was created in
//! during this session, else the user's home folder. It is session memory only: the
//! recent list (`storage`) records workspaces, not parent folders.
//!
//! `Decision:` Tab and Shift-Tab cycle through the form's own controls (Name → Location →
//! Browse → [Open it instead] → Create → Cancel) and wrap, so keyboard focus cannot fall
//! out of the dialog onto the workbench behind it.

use std::path::{Path, PathBuf};

use gpui::{
    DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Global, KeyDownEvent,
    PathPromptOptions, Render, Subscription, Task, WeakEntity,
};
use text_input::{TextInput, TextInputEvent};
use ui::components::form_field::cycle_focus;
use ui::prelude::*;

use crate::{ModalView, Workspace, WorkspaceOpen};

#[cfg(test)]
#[path = "new_workspace_form_tests.rs"]
mod tests;

/// The workspace file whose presence makes a folder a workspace (PRD §6.1).
const WORKSPACE_FILE: &str = "pustman.json";

/// The folder name a name with no ASCII letters or digits gets.
const FALLBACK_FOLDER_NAME: &str = "workspace";

/// The stable field names [`FormField`] derives its test selectors from.
pub const NAME_FIELD: &str = "new-workspace-name";
pub const LOCATION_FIELD: &str = "new-workspace-location";

/// Test hooks for the form's controls.
pub const CREATE_BUTTON_SELECTOR: &str = "new-workspace-create";
pub const CANCEL_BUTTON_SELECTOR: &str = "new-workspace-cancel";
pub const BROWSE_BUTTON_SELECTOR: &str = "new-workspace-browse";
pub const OPEN_INSTEAD_BUTTON_SELECTOR: &str = "new-workspace-open-instead";
pub const TARGET_PATH_SELECTOR: &str = "new-workspace-target";
pub const FAILURE_SELECTOR: &str = "new-workspace-failure";

/// The messages the form shows inline.
pub const NAME_REQUIRED: &str = "Enter a name.";
pub const LOCATION_REQUIRED: &str = "Choose the folder to create the workspace in.";
pub const LOCATION_MISSING: &str = "This folder does not exist. Choose another or create it first.";
pub const LOCATION_NOT_A_FOLDER: &str = "This is a file, not a folder. Choose a folder.";
pub const LOCATION_READ_ONLY: &str = "This folder is read-only. Choose one you can write to.";
pub const LOCATION_UNREADABLE: &str = "This folder could not be read.";
const LOCATION_HINT: &str = "The parent folder. Pustman creates the workspace folder inside it.";

/// The parent folder of the last workspace created this session (the module docs).
#[derive(Default)]
struct LastLocation(Option<PathBuf>);

impl Global for LastLocation {}

/// Open the New Workspace form over `workspace` (the `CreateWorkspace` answer). A
/// window without its open-workspace state has nowhere to create into and is left as is.
pub fn open(workspace: &Entity<Workspace>, window: &mut Window, cx: &mut App) {
    let Some(opener) = WorkspaceOpen::for_window(workspace.entity_id(), cx) else {
        return;
    };
    let opener = opener.downgrade();
    workspace.update(cx, |workspace, cx| {
        if workspace
            .modal_layer()
            .active_modal::<NewWorkspaceForm>()
            .is_some()
        {
            // Already open: asking again must not toggle it away under the user.
            return;
        }
        workspace.toggle_modal(window, cx, |window, cx| {
            NewWorkspaceForm::new(opener, window, cx)
        });
    });
}

/// What the background check found about the location the user typed.
#[derive(Clone, Debug, PartialEq, Eq)]
enum LocationCheck {
    /// The folder exists and takes writes.
    Usable,
    Missing,
    NotAFolder,
    ReadOnly,
    Unreadable,
}

/// One finished background check, for the text it was run against.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Probe {
    location: PathBuf,
    target: Option<PathBuf>,
    check: LocationCheck,
    /// The target folder already holds a `pustman.json`.
    target_is_workspace: bool,
}

/// The New Workspace dialog.
pub struct NewWorkspaceForm {
    opener: WeakEntity<WorkspaceOpen>,
    name: Entity<TextInput>,
    location: Entity<TextInput>,
    browse_focus: FocusHandle,
    open_instead_focus: FocusHandle,
    create_focus: FocusHandle,
    cancel_focus: FocusHandle,
    /// The name field was edited (or Create was tried), so an empty name is an error
    /// worth saying rather than the form's starting state.
    name_touched: bool,
    probe: Option<Probe>,
    probing: Option<Task<()>>,
    browsing: Option<Task<()>>,
    creating: bool,
    /// Why the last Create failed, shown inline until the next edit.
    failure: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl NewWorkspaceForm {
    fn new(opener: WeakEntity<WorkspaceOpen>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| {
            TextInput::new(window, cx)
                .with_placeholder(SharedString::new_static("e.g. Commerce API"))
        });
        let location = cx.new(|cx| TextInput::new(window, cx));
        let default_location = cx
            .try_global::<LastLocation>()
            .and_then(|last| last.0.clone())
            .or_else(home_folder);
        if let Some(default_location) = default_location {
            let text = default_location.to_string_lossy().into_owned();
            location.update(cx, |location, cx| location.set_text(&text, cx));
        }
        let subscriptions = vec![
            cx.subscribe_in(&name, window, |form, _input, event, window, cx| {
                form.on_field_event(*event, true, window, cx);
            }),
            cx.subscribe_in(&location, window, |form, _input, event, window, cx| {
                form.on_field_event(*event, false, window, cx);
            }),
        ];
        let mut form = Self {
            opener,
            name,
            location,
            browse_focus: cx.focus_handle(),
            open_instead_focus: cx.focus_handle(),
            create_focus: cx.focus_handle(),
            cancel_focus: cx.focus_handle(),
            name_touched: false,
            probe: None,
            probing: None,
            browsing: None,
            creating: false,
            failure: None,
            _subscriptions: subscriptions,
        };
        form.check_location(cx);
        form
    }

    /// A field changed (re-check) or the user pressed Enter in it (submit).
    fn on_field_event(
        &mut self,
        event: TextInputEvent,
        is_name: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextInputEvent::Changed => {
                if is_name {
                    self.name_touched = true;
                }
                self.failure = None;
                self.check_location(cx);
                cx.notify();
            }
            TextInputEvent::Submitted => self.submit(window, cx),
        }
    }

    /// The Name and Location fields — how the open path's own tests fill the form.
    #[cfg(test)]
    pub(crate) fn fields(&self) -> (Entity<TextInput>, Entity<TextInput>) {
        (self.name.clone(), self.location.clone())
    }

    /// The trimmed name the user typed.
    fn name_text(&self, cx: &App) -> String {
        self.name.read(cx).text().trim().to_owned()
    }

    /// The location the user typed, with a leading `~` meaning the home folder.
    fn location_path(&self, cx: &App) -> Option<PathBuf> {
        let text = self.location.read(cx).text();
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if text == "~" {
            return home_folder();
        }
        if let Some(rest) = text.strip_prefix("~/")
            && let Some(home) = home_folder()
        {
            return Some(home.join(rest));
        }
        Some(PathBuf::from(text))
    }

    /// The folder Create makes: `<location>/<slug of name>`.
    pub fn target(&self, cx: &App) -> Option<PathBuf> {
        let location = self.location_path(cx)?;
        let name = self.name_text(cx);
        if name.is_empty() {
            return None;
        }
        Some(location.join(folder_name(&name)))
    }

    /// Re-run the background location check for the current text. The previous check is
    /// dropped (cancelled): only the answer for what is on screen may land.
    fn check_location(&mut self, cx: &mut Context<Self>) {
        let Some(location) = self.location_path(cx) else {
            self.probe = None;
            self.probing = None;
            return;
        };
        let target = self.target(cx);
        if self
            .probe
            .as_ref()
            .is_some_and(|probe| probe.location == location && probe.target == target)
        {
            return;
        }
        self.probe = None;
        self.probing = Some(cx.spawn(async move |form, cx| {
            let probe = cx
                .background_spawn(async move { probe_location(location, target) })
                .await;
            // The form closed while the check ran; nobody is left to show it to.
            form.update(cx, |form, cx| {
                form.probe = Some(probe);
                form.probing = None;
                cx.notify();
            })
            .ok();
        }));
    }

    /// The inline error under Name, if any.
    fn name_error(&self, cx: &App) -> Option<&'static str> {
        (self.name_touched && self.name_text(cx).is_empty()).then_some(NAME_REQUIRED)
    }

    /// The inline error under Location, if any.
    fn location_error(&self, cx: &App) -> Option<&'static str> {
        if self.location_path(cx).is_none() {
            return Some(LOCATION_REQUIRED);
        }
        match self.probe.as_ref()?.check {
            LocationCheck::Usable => None,
            LocationCheck::Missing => Some(LOCATION_MISSING),
            LocationCheck::NotAFolder => Some(LOCATION_NOT_A_FOLDER),
            LocationCheck::ReadOnly => Some(LOCATION_READ_ONLY),
            LocationCheck::Unreadable => Some(LOCATION_UNREADABLE),
        }
    }

    /// The target already holds a workspace (offer "Open it instead").
    fn existing_workspace(&self, cx: &App) -> Option<PathBuf> {
        let probe = self.probe.as_ref()?;
        if !probe.target_is_workspace {
            return None;
        }
        probe
            .target
            .clone()
            .filter(|target| Some(target) == self.target(cx).as_ref())
    }

    /// Every check passed for the text on screen, and no create is in flight.
    pub fn can_create(&self, cx: &App) -> bool {
        !self.creating
            && !self.name_text(cx).is_empty()
            && self.location_error(cx).is_none()
            && self
                .probe
                .as_ref()
                .is_some_and(|probe| probe.target == self.target(cx) && !probe.target_is_workspace)
    }

    /// Create, if valid (Enter and the Create button). An invalid form says why inline.
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.name_touched = true;
        if !self.can_create(cx) {
            cx.notify();
            return;
        }
        let (Some(target), Some(location)) = (self.target(cx), self.location_path(cx)) else {
            return;
        };
        let name = self.name_text(cx);
        let form = cx.weak_entity();
        let window_handle = window.window_handle();
        let started = self.opener.update(cx, |opener, cx| {
            opener.create_named(
                target,
                name,
                move |finished, cx| match finished {
                    Ok(()) => {
                        cx.default_global::<LastLocation>().0 = Some(location);
                        form.update(cx, |_form, cx| cx.emit(DismissEvent)).ok();
                        true
                    }
                    Err(reason) => {
                        let shown = window_handle.update(cx, |_root, _window, cx| {
                            form.update(cx, |form, cx| {
                                form.creating = false;
                                form.failure = Some(reason);
                                cx.notify();
                            })
                            .is_ok()
                        });
                        matches!(shown, Ok(true))
                    }
                },
                cx,
            );
        });
        if started.is_err() {
            self.failure = Some(SharedString::new_static(
                "This window can no longer open a workspace.",
            ));
        } else {
            self.creating = true;
            self.failure = None;
        }
        cx.notify();
    }

    /// Open the existing workspace at the target instead of creating one.
    fn open_instead(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.existing_workspace(cx) else {
            return;
        };
        if self
            .opener
            .update(cx, |opener, cx| opener.open_folder(target, cx))
            .is_ok()
        {
            cx.emit(DismissEvent);
        }
    }

    /// Browse…: the OS folder picker fills Location.
    fn browse(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::new_static(
                "Choose where to create the workspace",
            )),
        });
        self.browsing = Some(cx.spawn(async move |form, cx| {
            let chosen = match receiver.await {
                Ok(Ok(paths)) => paths.and_then(|paths| paths.into_iter().next()),
                Ok(Err(error)) => {
                    form.update(cx, |form, cx| {
                        form.failure =
                            Some(format!("The folder dialog could not be opened — {error}").into());
                        cx.notify();
                    })
                    .ok();
                    return;
                }
                // The dialog went away with its window.
                Err(_canceled) => return,
            };
            let Some(chosen) = chosen else {
                // Cancelled: the field keeps what it had.
                return;
            };
            form.update(cx, |form, cx| {
                let text = chosen.to_string_lossy().into_owned();
                form.location
                    .update(cx, |location, cx| location.set_text(&text, cx));
            })
            .ok();
        }));
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    /// The form's tab stops, in order (the module docs).
    fn tab_order(&self, cx: &App) -> Vec<FocusHandle> {
        let mut order = vec![
            self.name.read(cx).focus_handle(cx),
            self.location.read(cx).focus_handle(cx),
            self.browse_focus.clone(),
        ];
        if self.existing_workspace(cx).is_some() {
            order.push(self.open_instead_focus.clone());
        }
        if self.can_create(cx) {
            order.push(self.create_focus.clone());
        }
        order.push(self.cancel_focus.clone());
        order
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let order = self.tab_order(cx);
        if cycle_focus(&order, event, window, cx) {
            cx.stop_propagation();
        }
    }
}

impl Render for NewWorkspaceForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let target = self.target(cx);
        let can_create = self.can_create(cx);
        let existing = self.existing_workspace(cx);
        let name_error = self.name_error(cx);
        let location_error = self.location_error(cx);
        let footer_hint = if self.creating {
            "Creating…"
        } else if can_create {
            "Enter to create"
        } else {
            "Fix the fields above"
        };

        let name_field = FormField::new(NAME_FIELD, "Name")
            .content(self.name.clone())
            .map(|field| match name_error {
                Some(error) => field.error(error),
                None => field,
            });
        let location_field = FormField::new(LOCATION_FIELD, "Location")
            .content(
                h_flex()
                    .w_full()
                    .min_w_0()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .child(div().flex_1().min_w_0().child(self.location.clone()))
                    .child(
                        div()
                            .flex_none()
                            .debug_selector(|| BROWSE_BUTTON_SELECTOR.to_owned())
                            .child(
                                Button::new("new-workspace-browse", "Browse…")
                                    .style(ButtonStyle::Outlined)
                                    .focus_handle(self.browse_focus.clone())
                                    .on_click(cx.listener(|form, _event, _window, cx| {
                                        form.browse(cx);
                                    })),
                            ),
                    ),
            )
            .map(|field| match location_error {
                Some(error) => field.error(error),
                None => field.hint(LOCATION_HINT),
            });

        let target_text: SharedString = match &target {
            Some(target) => target.to_string_lossy().into_owned().into(),
            None => SharedString::new_static("Enter a name and a location"),
        };

        let body = v_flex()
            .w_full()
            .gap(DynamicSpacing::Base08.rems(cx))
            .child(name_field)
            .child(location_field)
            .child(
                v_flex()
                    .id("new-workspace-target")
                    .role(gpui::Role::Group)
                    .aria_label(SharedString::new_static("Will be created at"))
                    .gap(DynamicSpacing::Base02.rems(cx))
                    .child(
                        Label::new("Will be created at")
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .child(
                        div()
                            .debug_selector(|| TARGET_PATH_SELECTOR.to_owned())
                            .min_w_0()
                            .child(Label::new(target_text).size(LabelSize::Small)),
                    ),
            )
            .when_some(existing, |body, existing| {
                body.child(
                    h_flex()
                        .w_full()
                        .gap(DynamicSpacing::Base04.rems(cx))
                        .child(
                            div().flex_1().min_w_0().child(
                                Banner::new(
                                    "new-workspace-exists",
                                    format!("{} is already a workspace.", existing.display()),
                                )
                                .severity(Severity::Warning),
                            ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .debug_selector(|| OPEN_INSTEAD_BUTTON_SELECTOR.to_owned())
                                .child(
                                    Button::new("new-workspace-open-instead", "Open it instead")
                                        .style(ButtonStyle::Outlined)
                                        .focus_handle(self.open_instead_focus.clone())
                                        .on_click(cx.listener(|form, _event, _window, cx| {
                                            form.open_instead(cx);
                                        })),
                                ),
                        ),
                )
            })
            .when_some(self.failure.clone(), |body, failure| {
                body.child(
                    div().debug_selector(|| FAILURE_SELECTOR.to_owned()).child(
                        Banner::new("new-workspace-failure", failure).severity(Severity::Error),
                    ),
                )
            });

        let footer = h_flex()
            .w_full()
            .justify_end()
            .items_center()
            .gap(DynamicSpacing::Base04.rems(cx))
            .child(
                div().flex_1().min_w_0().child(
                    Label::new(footer_hint)
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                ),
            )
            .child(
                div()
                    .debug_selector(|| CANCEL_BUTTON_SELECTOR.to_owned())
                    .child(
                        Button::new("new-workspace-cancel", "Cancel")
                            .style(ButtonStyle::Outlined)
                            .focus_handle(self.cancel_focus.clone())
                            .on_click(cx.listener(|form, _event, _window, cx| form.cancel(cx))),
                    ),
            )
            .child(
                div()
                    .debug_selector(|| CREATE_BUTTON_SELECTOR.to_owned())
                    .child(
                        Button::new("new-workspace-create", "Create workspace")
                            .style(ButtonStyle::Filled)
                            .disabled(!can_create)
                            .loading(self.creating)
                            .focus_handle(self.create_focus.clone())
                            .on_click(cx.listener(|form, _event, window, cx| {
                                form.submit(window, cx);
                            })),
                    ),
            );

        div()
            .id("new-workspace-form")
            .w(px_to_rems(FORM_WIDTH_PX))
            .max_w_full()
            .on_key_down(cx.listener(Self::on_key_down))
            .child(Modal::new("New workspace").content(body).footer(footer))
    }
}

/// The form's width: the design pack's dialog width, inside [`Modal`]'s own cap.
const FORM_WIDTH_PX: f32 = 480.;

impl Focusable for NewWorkspaceForm {
    /// Name has focus when the form opens.
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.name.read(cx).focus_handle(cx)
    }
}

impl EventEmitter<DismissEvent> for NewWorkspaceForm {}

impl ModalView for NewWorkspaceForm {}

/// The folder name for a workspace called `name`: its slug, like an item's file name.
fn folder_name(name: &str) -> String {
    let slug = workspace_store::slugify(name);
    if slug.is_empty() {
        FALLBACK_FOLDER_NAME.to_owned()
    } else {
        slug
    }
}

/// The user's home folder, if the session names one.
fn home_folder() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

/// The location check, off the foreground (the module docs).
fn probe_location(location: PathBuf, target: Option<PathBuf>) -> Probe {
    let check = match std::fs::metadata(&location) {
        Ok(metadata) if !metadata.is_dir() => LocationCheck::NotAFolder,
        Ok(metadata) if metadata.permissions().readonly() => LocationCheck::ReadOnly,
        Ok(_) => LocationCheck::Usable,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => LocationCheck::Missing,
        Err(_) => LocationCheck::Unreadable,
    };
    let target_is_workspace = target.as_deref().is_some_and(holds_workspace);
    Probe {
        location,
        target,
        check,
        target_is_workspace,
    }
}

fn holds_workspace(folder: &Path) -> bool {
    folder.join(WORKSPACE_FILE).exists()
}
