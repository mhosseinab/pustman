//! The Manage Cookies dialog (PRD REQ-HTTP-08): a modal listing the workspace's cookie
//! jar grouped by domain, where a cookie can be added, edited or deleted and one whole
//! domain cleared — every change persisted through the [`CookieJar`] API on a background
//! task.
//!
//! Add and edit go through the inline [`crate::cookie_form`] panel (its module docs hold
//! the form's decisions): the "Add cookie" control and each row's Edit control put it
//! where the list is, and a save that lands rebuilds the rows from the jar.
//!
//! [`CookieJar`] and `http_engine::cookie_persistence` do the whole job — listing,
//! deleting, clearing, saving — so this file is the dialog and never another jar. The
//! rows are grouped by domain and sorted ([`CookieJar::list`] sorts by (domain, path,
//! name); a `HashMap`'s order would make the rows jitter), and a row's identity is its
//! (domain, path, name) — never its position, which a delete shifts
//! (`.agents/rules/ui-render.md`).
//!
//! `Decision:` [`CookieDialogInputs`] is an app-global the app's wire installs with
//! [`install_inputs`] — the `environment_picker::selector` shape for the same "the top
//! crate owns the inputs" problem: this crate holds neither the workspace's
//! `Arc<Mutex<LocalStore>>` nor its jar. `cookies::Manage` opens the dialog when the
//! inputs exist and shows the same kind of "not wired" toast when they do not, so a
//! click never loses a change silently.
//!
//! `Decision:` a refused change and a failed save both show inline in the dialog rather
//! than as a toast: the dialog is still open on exactly the rows the change failed on,
//! which is where the user is looking (AGENTS.md: errors reach the user, never only a
//! log).
//!
//! Cookie values are secrets (PRD §7): nothing here logs or Debug-prints one, and
//! [`DisplayedRow`] carries no `Debug` — its `value` field can hold one.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use gpui::{
    App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Global, Render,
    Subscription, Task, Window,
};
use http_engine::{CookieEntry, CookieError, CookieJar, save_jar};
use pustman_actions::cookies::Manage;
use storage::LocalStore;
use time::format_description::well_known::Rfc2822;
use ui::prelude::*;
use workspace::{ModalView, Workspace};

use crate::child_id;
use crate::cookie_form::{CookieForm, CookieFormEvent};
use crate::table_cells::{ColumnWidth, plain_cell};

#[cfg(test)]
#[path = "cookie_dialog_tests.rs"]
mod tests;

/// One row's height at the default scale. Rows are uniform because the virtualized list
/// measures one row for all of them (the `history_list` contract).
const ROW_HEIGHT_PX: f32 = 40.;

/// The width of a row's action column — the edit and delete buttons' box.
const ACTION_COLUMN_PX: f32 = 68.;

/// How wide each of a cookie row's four cells is: the row's free width, shared evenly.
const CELL_WIDTH: ColumnWidth = ColumnWidth::Share(1.);

/// How many rows the list shows before it scrolls inside the dialog.
const MAX_VISIBLE_ROWS: u16 = 12;

/// The dialog's width: room for the four cells of a cookie row to read.
const DIALOG_WIDTH_PX: f32 = 720.;

/// Where a cookie's expiry cell shows it dies with the session (RFC 6265).
const SESSION_LABEL: &str = "Session";

/// What a cell shows where an attribute has no value.
const ABSENT: &str = "—";

/// The one message the opener shows when the app has not installed [`CookieDialogInputs`]
/// (AGENTS.md: errors reach the user, never only a log).
const NOT_WIRED_MESSAGE: &str =
    "The Manage Cookies dialog cannot save changes — the local store is not connected";

/// What a jar lock a panicked change left poisoned shows — the same shape
/// `cookie_persistence` reports its lock failures with.
const JAR_LOCK_MESSAGE: &str =
    "The cookie jar is unavailable — it was left mid-operation by a failed change";

/// What [`cookies::Manage`](Manage) cannot know on its own: the workspace the jar belongs
/// to, the app's local store the jar persists through, and the jar itself. See the
/// module docs for the app-global caveat this shape carries.
#[derive(Clone)]
pub struct CookieDialogInputs {
    /// The workspace the jar and its saved rows belong to.
    pub workspace_id: String,
    /// The app's local store `save_jar` persists the jar into.
    pub store: Arc<Mutex<LocalStore>>,
    /// The workspace's cookie jar the dialog lists and changes.
    pub jar: Arc<Mutex<CookieJar>>,
}

impl Global for CookieDialogInputs {}

/// Install the inputs the opener runs with; the next [`Manage`] uses them.
pub fn install_inputs(cx: &mut App, inputs: CookieDialogInputs) {
    cx.set_global(inputs);
}

/// One row of the dialog's list: a domain group header or one cookie.
enum Row {
    /// A domain header: the domain, how many cookies sit under it, and the focus handle
    /// of its "Clear" button.
    Domain {
        domain: String,
        cookie_count: usize,
        clear_focus: FocusHandle,
    },
    /// One cookie: the entry, its expiry as the row shows it, and its edit and delete
    /// buttons' focus handles.
    Cookie {
        entry: CookieEntry,
        expires_text: String,
        edit_focus: FocusHandle,
        delete_focus: FocusHandle,
    },
}

/// One row as the user sees it: exactly the strings [`Render`] puts in its cells, so a
/// test can assert on the display without re-deriving it. Carries no [`Debug`](std::fmt::Debug)
/// on purpose — [`Self::value`] holds a cookie value, which is a secret (PRD §7).
pub struct DisplayedRow {
    /// The row's domain: a group header's own text, or the group a cookie row sits under
    /// (its domain shows in the header, not in every row).
    pub domain: String,
    /// The Name cell: the cookie's name, or a group header's count ("2 cookies").
    pub name: String,
    /// The Value cell — a cookie value, never logged or debug-printed.
    pub value: String,
    pub path: String,
    pub expires: String,
    /// Whether this is a domain group header rather than a cookie row.
    pub group_header: bool,
}

/// The Manage Cookies dialog: the jar's rows grouped by domain, the inline error a
/// refused change or failed save leaves, and the save in flight.
pub struct CookieDialog {
    focus_handle: FocusHandle,
    workspace_id: String,
    store: Arc<Mutex<LocalStore>>,
    jar: Arc<Mutex<CookieJar>>,
    rows: Arc<Vec<Row>>,
    /// Why the last change did not happen or did not persist, shown above the rows.
    error: Option<SharedString>,
    /// The in-flight save. Stored so a second change replaces (cancels) the save it
    /// supersedes: a save writes the whole snapshot, so only the newest one needs to
    /// finish (AGENTS.md › Dropping a `Task` cancels it).
    save_task: Option<Task<()>>,
    /// The add/edit form open over the list, while one is open (REQ-HTTP-08).
    form: Option<Entity<CookieForm>>,
    /// The "Add cookie" button's focus handle: its slot in the tab order.
    add_focus: FocusHandle,
    /// Kept so the form's Save and Cancelled events keep reaching us; dropping it would
    /// silently strand the form (AGENTS.md › GPUI).
    form_subscription: Option<Subscription>,
}

impl CookieDialog {
    /// A dialog over `inputs`' jar, listing it as it stands right now. Listing is an
    /// in-memory read (the jar is an `Arc<Mutex<CookieJar>>`), so no IO runs here.
    pub fn new(inputs: CookieDialogInputs, cx: &mut Context<Self>) -> Self {
        let CookieDialogInputs {
            workspace_id,
            store,
            jar,
        } = inputs;
        let mut dialog = Self {
            focus_handle: cx.focus_handle(),
            workspace_id,
            store,
            jar,
            rows: Arc::new(Vec::new()),
            error: None,
            save_task: None,
            form: None,
            add_focus: cx.focus_handle(),
            form_subscription: None,
        };
        dialog.reload_rows(cx);
        dialog
    }

    /// Every row as the user sees it, in display order.
    pub fn displayed_rows(&self) -> Vec<DisplayedRow> {
        self.rows
            .iter()
            .map(|row| match row {
                Row::Domain {
                    domain,
                    cookie_count,
                    ..
                } => DisplayedRow {
                    domain: domain.clone(),
                    name: cookie_count_label(*cookie_count),
                    value: String::new(),
                    path: String::new(),
                    expires: String::new(),
                    group_header: true,
                },
                Row::Cookie {
                    entry,
                    expires_text,
                    ..
                } => DisplayedRow {
                    domain: entry.domain.clone(),
                    name: entry.name.clone(),
                    value: entry.value.clone(),
                    path: entry.path.clone(),
                    expires: expires_text.clone(),
                    group_header: false,
                },
            })
            .collect()
    }

    /// The delete control of the (domain, path, name) row, as the keyboard reaches it:
    /// the dialog hands the control its tab order through this handle.
    pub fn delete_focus_handle(&self, domain: &str, path: &str, name: &str) -> Option<FocusHandle> {
        self.rows.iter().find_map(|row| match row {
            Row::Cookie {
                entry,
                delete_focus,
                ..
            } if entry.domain == domain && entry.path == path && entry.name == name => {
                Some(delete_focus.clone())
            }
            _ => None,
        })
    }

    /// The edit control of the (domain, path, name) row, as the keyboard reaches it —
    /// [`Self::delete_focus_handle`]'s twin.
    pub fn edit_focus_handle(&self, domain: &str, path: &str, name: &str) -> Option<FocusHandle> {
        self.rows.iter().find_map(|row| match row {
            Row::Cookie {
                entry, edit_focus, ..
            } if entry.domain == domain && entry.path == path && entry.name == name => {
                Some(edit_focus.clone())
            }
            _ => None,
        })
    }

    /// The "Add cookie" control, as the keyboard reaches it: the dialog hands the
    /// control its tab order through this handle.
    pub fn add_focus_handle(&self) -> FocusHandle {
        self.add_focus.clone()
    }

    /// The add/edit form open where the list is, while one is open (REQ-HTTP-08).
    pub fn form(&self) -> Option<Entity<CookieForm>> {
        self.form.clone()
    }

    /// Rebuild the listed rows from the jar — on open and after every change.
    fn reload_rows(&mut self, cx: &mut Context<Self>) {
        // The handle is cloned out first so the lock never borrows `self` while the
        // rows land on it (`.agents/rules/rust-code.md`).
        let jar_handle = Arc::clone(&self.jar);
        match jar_handle.lock() {
            Ok(jar) => self.rows = Arc::new(build_rows(&jar, cx)),
            Err(_) => {
                self.error = Some(SharedString::new_static(JAR_LOCK_MESSAGE));
            }
        }
    }

    /// Delete one cookie from the jar and persist what is left (REQ-HTTP-08). A refused
    /// change shows in the dialog banner — there is no form to show it in.
    fn delete_cookie(&mut self, entry: &CookieEntry, cx: &mut Context<Self>) {
        let (domain, path, name) = (entry.domain.clone(), entry.path.clone(), entry.name.clone());
        if let Err(message) = self.change_jar(move |jar| jar.delete(&domain, &path, &name), cx) {
            self.error = Some(message);
            cx.notify();
        }
    }

    /// Remove every cookie of one domain and persist what is left (REQ-HTTP-08).
    fn clear_domain(&mut self, domain: &str, cx: &mut Context<Self>) {
        let domain = domain.to_owned();
        if let Err(message) = self.change_jar(
            move |jar| {
                jar.clear_domain(&domain);
                Ok(())
            },
            cx,
        ) {
            self.error = Some(message);
            cx.notify();
        }
    }

    /// The "Add cookie" control: a blank form where the list is (REQ-HTTP-08).
    fn open_add_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = cx.new(|cx| CookieForm::add(window, cx));
        self.mount_form(form, window, cx);
    }

    /// A row's Edit control: the form over that cookie (REQ-HTTP-08).
    fn open_edit_form(&mut self, entry: &CookieEntry, window: &mut Window, cx: &mut Context<Self>) {
        let form = cx.new(|cx| CookieForm::edit(entry.clone(), window, cx));
        self.mount_form(form, window, cx);
    }

    /// Put `form` where the list is, with the keyboard in its first field.
    fn mount_form(
        &mut self,
        form: Entity<CookieForm>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.form_subscription = Some(cx.subscribe(&form, Self::on_form_event));
        self.form = Some(form.clone());
        self.error = None;
        let first_field = form.read(cx).focus_handle(cx);
        window.focus(&first_field, cx);
        cx.notify();
    }

    /// The form's Save and Cancel (REQ-HTTP-08): a save goes through the jar and the
    /// same background `save_jar` every other change uses, and closes the form when it
    /// lands; a refused save stays open on exactly the fields it failed on.
    fn on_form_event(
        &mut self,
        _form: Entity<CookieForm>,
        event: &CookieFormEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            CookieFormEvent::Cancelled => self.close_form(cx),
            CookieFormEvent::Save { original, entry } => {
                let entry = entry.clone();
                let outcome = match original {
                    Some(identity) => self.change_jar(
                        move |jar| {
                            jar.edit(&identity.domain, &identity.path, &identity.name, entry)
                        },
                        cx,
                    ),
                    None => self.change_jar(move |jar| jar.add(entry), cx),
                };
                match outcome {
                    // The rows are rebuilt and the save is in flight: the form's job is
                    // done, the list comes back.
                    Ok(()) => self.close_form(cx),
                    // The jar refused the change; its message shows where the change was
                    // typed (AGENTS.md: errors reach the user).
                    Err(message) => self.show_form_error(message, cx),
                }
            }
        }
    }

    /// Take the form down: the list shows again — unchanged (Cancel) or rebuilt from the
    /// jar the save changed (Save).
    fn close_form(&mut self, cx: &mut Context<Self>) {
        self.form = None;
        self.form_subscription = None;
        cx.notify();
    }

    /// Put `message` in the form's inline error — or the dialog banner if the form is
    /// already gone. The handle is strong, so a present form always takes it.
    fn show_form_error(&mut self, message: SharedString, cx: &mut Context<Self>) {
        match self.form.clone() {
            Some(form) => form.update(cx, |form, cx| form.show_error(message, cx)),
            None => self.error = Some(message),
        }
        cx.notify();
    }

    /// Apply `change` to the jar, rebuild the rows from it and save it on a background
    /// task. On refusal it returns the message to show — the jar's `CookieError` Display,
    /// which names the cookie and never its value (PRD §7), or the poisoned-lock message
    /// — because the list-mode changes show it in [`Self::error`] while a form save shows
    /// it in the form that asked for it. A failed save always lands in [`Self::error`]
    /// (inline in the dialog, never only logged; AGENTS.md).
    fn change_jar(
        &mut self,
        change: impl FnOnce(&mut CookieJar) -> Result<(), CookieError>,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        // Cloned out first so the lock never borrows `self` while the rows and the error
        // land on it (`.agents/rules/rust-code.md`).
        let jar_handle = Arc::clone(&self.jar);
        let mut jar = match jar_handle.lock() {
            Ok(jar) => jar,
            Err(_) => return Err(SharedString::new_static(JAR_LOCK_MESSAGE)),
        };
        if let Err(refusal) = change(&mut jar) {
            return Err(SharedString::from(refusal.to_string()));
        }
        self.rows = Arc::new(build_rows(&jar, cx));
        self.error = None;
        // The snapshot `save_jar` takes happens right here, on this thread (cheap, no
        // IO); every SQLite call runs inside its task (ADR-0002).
        let write = save_jar(
            cx.background_executor(),
            Arc::clone(&self.store),
            self.workspace_id.clone(),
            &jar,
        );
        drop(jar);
        cx.notify();
        self.save_task = Some(cx.spawn(async move |this, cx| {
            if let Err(failure) = write.await {
                // `cookie_persistence`'s failure text is fixed and value-free (PRD §7).
                let message = failure.to_string();
                if this
                    .update(cx, |this, cx| {
                        this.error = Some(SharedString::from(message));
                        cx.notify();
                    })
                    .is_err()
                {
                    // Handled: the dialog closed with its window before the save
                    // failed; there is nobody left to tell.
                }
            }
        }));
        Ok(())
    }

    /// One row's content — the wrapper [`VirtualList`] draws around it carries the row's
    /// id and test selector.
    fn render_row(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let rows = Arc::clone(&self.rows);
        let Some(row) = rows.get(index) else {
            // The list draws the same snapshot its row count comes from, so an
            // out-of-range index cannot occur — and must not panic if it ever does.
            return div().into_any_element();
        };
        let row_id = row_key(row);
        match row {
            Row::Domain {
                domain,
                cookie_count,
                clear_focus,
            } => self.render_domain_header(&row_id, domain, *cookie_count, clear_focus, cx),
            Row::Cookie {
                entry,
                expires_text,
                edit_focus,
                delete_focus,
            } => self.render_cookie_row(&row_id, entry, expires_text, edit_focus, delete_focus, cx),
        }
    }

    /// One domain group header: the domain, how many cookies sit under it, and the
    /// "Clear" button that removes all of them.
    fn render_domain_header(
        &self,
        row_id: &ElementId,
        domain: &str,
        cookie_count: usize,
        clear_focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cleared_domain = domain.to_owned();
        let weak = cx.weak_entity();
        h_flex()
            .w_full()
            .h(px_to_rems(ROW_HEIGHT_PX))
            .min_w_0()
            .items_center()
            .gap(DynamicSpacing::Base04.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(
                div().flex_1().min_w_0().child(
                    Label::new(domain.to_owned())
                        .size(LabelSize::Small)
                        .truncate(),
                ),
            )
            .child(
                div().flex_none().child(
                    Label::new(cookie_count_label(cookie_count))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                ),
            )
            .child(
                Button::new(child_id(row_id, "clear"), "Clear")
                    .size(ButtonSize::Default)
                    .tooltip(Tooltip::text(format!("Remove every cookie for {domain}")))
                    .focus_handle(clear_focus.clone())
                    .on_click(move |_event, _window, cx| {
                        if weak
                            .update(cx, |dialog, cx| dialog.clear_domain(&cleared_domain, cx))
                            .is_err()
                        {
                            // Handled: the dialog went away with its window while the
                            // button was up; there is nothing left to clear.
                        }
                    }),
            )
            .into_any_element()
    }

    /// One cookie row: the name, value, path and expiry cells, and its edit and delete
    /// buttons.
    fn render_cookie_row(
        &self,
        row_id: &ElementId,
        entry: &CookieEntry,
        expires_text: &str,
        edit_focus: &FocusHandle,
        delete_focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let deleted = entry.clone();
        let edited = entry.clone();
        let delete_weak = cx.weak_entity();
        let edit_weak = cx.weak_entity();
        h_flex()
            .w_full()
            .h(px_to_rems(ROW_HEIGHT_PX))
            .min_w_0()
            .items_center()
            .gap(DynamicSpacing::Base04.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .border_b_1()
            .border_color(cx.theme().colors().border_variant)
            .child(plain_cell(
                child_id(row_id, "name"),
                entry.name.clone().into(),
                CELL_WIDTH,
                cx,
            ))
            .child(plain_cell(
                child_id(row_id, "value"),
                entry.value.clone().into(),
                CELL_WIDTH,
                cx,
            ))
            .child(plain_cell(
                child_id(row_id, "path"),
                entry.path.clone().into(),
                CELL_WIDTH,
                cx,
            ))
            .child(plain_cell(
                child_id(row_id, "expires"),
                expires_text.into(),
                CELL_WIDTH,
                cx,
            ))
            .child(
                h_flex()
                    .flex_none()
                    .w(px_to_rems(ACTION_COLUMN_PX))
                    .gap(DynamicSpacing::Base02.rems(cx))
                    .child(
                        IconButton::new(
                            child_id(row_id, "edit"),
                            IconName::Pencil,
                            Tooltip::text(format!("Edit cookie {}", entry.name)),
                        )
                        .focus_handle(edit_focus.clone())
                        .on_click(move |_event, window, cx| {
                            if edit_weak
                                .update(cx, |dialog, cx| dialog.open_edit_form(&edited, window, cx))
                                .is_err()
                            {
                                // Handled: the dialog went away with its window while
                                // the button was up; there is nothing left to edit.
                            }
                        }),
                    )
                    .child(
                        IconButton::new(
                            child_id(row_id, "delete"),
                            IconName::Trash,
                            Tooltip::text(format!("Delete cookie {}", entry.name)),
                        )
                        .focus_handle(delete_focus.clone())
                        .on_click(move |_event, _window, cx| {
                            if delete_weak
                                .update(cx, |dialog, cx| dialog.delete_cookie(&deleted, cx))
                                .is_err()
                            {
                                // Handled: the dialog went away with its window while the
                                // button was up; there is nothing left to delete.
                            }
                        }),
                    ),
            )
            .into_any_element()
    }
}

impl Render for CookieDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // One base id per entity, so two dialogs in one frame never share element ids.
        let base = ElementId::from(("cookie-dialog", cx.entity_id()));
        let rows = Arc::clone(&self.rows);
        let error = self.error.clone();
        let form = self.form.clone();
        let content = v_flex()
            .w_full()
            .min_w_0()
            .gap(DynamicSpacing::Base04.rems(cx))
            .map(|this| match error {
                Some(message) => this.child(
                    Label::new(message)
                        .size(LabelSize::Small)
                        .color(Color::Error),
                ),
                None => this,
            })
            .map(|this| match form {
                // The form is where the list is while it is open (its module docs'
                // Decision); the list comes back — refreshed from the jar — after a
                // save or a cancel.
                Some(form) => this.child(form),
                None if rows.is_empty() => this.child(empty_state(cx)),
                None => {
                    let key_rows = Arc::clone(&rows);
                    let key_base = base.clone();
                    this.child(column_headings(cx)).child(
                        div()
                            .w_full()
                            .h(list_height(rows.len()))
                            .child(VirtualList::new(
                                child_id(&base, "rows"),
                                rows.len(),
                                move |index| match key_rows.get(index) {
                                    Some(row) => row_key(row),
                                    None => child_id(&key_base, format!("row-{index}")),
                                },
                                cx.processor(|this, index: usize, _window, cx| {
                                    this.render_row(index, cx)
                                }),
                            )),
                    )
                }
            });
        Modal::new("Manage cookies")
            .max_width(px_to_rems(DIALOG_WIDTH_PX))
            .content(content)
            .footer(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap(DynamicSpacing::Base04.rems(cx))
                    .map(|this| match &self.form {
                        // The form is its own add flow (it has Save and Cancel): the
                        // list's "Add cookie" would only lose what is typed in it.
                        Some(_) => this,
                        None => this.child(
                            Button::new(child_id(&base, "add-cookie"), "Add cookie")
                                .start_icon(Icon::new(IconName::Plus))
                                .tooltip(Tooltip::text("Add a cookie to this workspace's jar"))
                                .focus_handle(self.add_focus.clone())
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.open_add_form(window, cx)
                                })),
                        ),
                    })
                    .child(
                        Button::new(child_id(&base, "close"), "Close").on_click(cx.listener(
                            |_this, _event, _window, cx| {
                                cx.emit(DismissEvent);
                            },
                        )),
                    ),
            )
    }
}

impl Focusable for CookieDialog {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for CookieDialog {}

impl ModalView for CookieDialog {}

/// Wire the dialog to [`cookies::Manage`](Manage): open it over the active workspace with
/// the [`CookieDialogInputs`] the app's wire installed. Registered as an app-level action
/// in `response_viewer::init`, the `environment_picker::selector` shape.
pub fn init(cx: &mut App) {
    cx.on_action(open_cookie_dialog);
}

/// Open the dialog in the active window. The dispatch happens inside the window's own
/// update and a window cannot be updated while it is on the update stack, so the open is
/// deferred — the shape `environment_picker::selector`'s opener uses for the same
/// situation.
fn open_cookie_dialog(_: &Manage, cx: &mut App) {
    let Some(window_handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        if window_handle
            .update(cx, |root, window, cx| {
                let Ok(workspace_handle) = root.downcast::<Workspace>() else {
                    return;
                };
                // Cloned out before the workspace update borrows `cx` again.
                let inputs = cx.try_global::<CookieDialogInputs>().cloned();
                workspace_handle.update(cx, |workspace, cx| {
                    let Some(inputs) = inputs else {
                        // Errors reach the user: a dialog that cannot save the change
                        // would lose it silently.
                        workspace.show_toast(NOT_WIRED_MESSAGE, Severity::Error, None, cx);
                        return;
                    };
                    workspace.toggle_modal(window, cx, |_window, cx| CookieDialog::new(inputs, cx));
                });
            })
            .is_err()
        {
            // The window closed between the keystroke and this callback; there is no
            // workspace left to open the dialog over.
        }
    });
}

/// The dialog's rows from the jar's cookies: one [`Row::Domain`] header per domain
/// followed by its cookies. [`CookieJar::list`] sorts by (domain, path, name) and the
/// map is a `BTreeMap`, so the row order is sorted end to end — never a `HashMap`
/// iteration order (`.agents/rules/rust-code.md`).
fn build_rows(jar: &CookieJar, cx: &mut App) -> Vec<Row> {
    let mut groups: BTreeMap<String, Vec<CookieEntry>> = BTreeMap::new();
    for entry in jar.list() {
        groups.entry(entry.domain.clone()).or_default().push(entry);
    }
    let mut rows = Vec::new();
    for (domain, entries) in groups {
        let cookie_count = entries.len();
        rows.push(Row::Domain {
            domain,
            cookie_count,
            clear_focus: cx.focus_handle(),
        });
        for entry in entries {
            let expires_text = expires_text(&entry);
            rows.push(Row::Cookie {
                entry,
                expires_text,
                edit_focus: cx.focus_handle(),
                delete_focus: cx.focus_handle(),
            });
        }
    }
    rows
}

/// A row's stable identity: the cookie's (domain, path, name) — what [`CookieJar::delete`]
/// takes and what stays put when other rows go (never a list index, which a delete
/// shifts). The header's is its domain. No cookie value ever enters an element id.
fn row_key(row: &Row) -> ElementId {
    match row {
        Row::Domain { domain, .. } => ElementId::from(format!("cookie-domain-{domain}")),
        Row::Cookie { entry, .. } => ElementId::from(format!(
            "cookie-{}-{}-{}",
            entry.domain, entry.path, entry.name
        )),
    }
}

/// When a cookie dies, as the row shows it: an RFC 2822 timestamp or [`SESSION_LABEL`].
fn expires_text(entry: &CookieEntry) -> String {
    match entry.expires {
        Some(when) => when
            .format(&Rfc2822)
            .unwrap_or_else(|_| String::from(ABSENT)),
        None => String::from(SESSION_LABEL),
    }
}

/// How many cookies a domain header announces.
fn cookie_count_label(cookie_count: usize) -> String {
    if cookie_count == 1 {
        String::from("1 cookie")
    } else {
        format!("{cookie_count} cookies")
    }
}

/// The list's height: one row per listed row, capped so a big jar scrolls inside the
/// dialog instead of stretching it. Rows are uniform, so the height is exact.
fn list_height(row_count: usize) -> Rems {
    // The count comes from user data (how many cookies a response set), so it clamps
    // before it converts (`.agents/rules/rust-code.md`).
    let visible = u16::try_from(row_count)
        .unwrap_or(u16::MAX)
        .clamp(1, MAX_VISIBLE_ROWS);
    px_to_rems(f32::from(visible) * ROW_HEIGHT_PX)
}

/// The column headings above the cookie rows (`cookies_tab`'s, same shape).
fn column_headings(cx: &App) -> impl IntoElement {
    h_flex()
        .w_full()
        .gap(DynamicSpacing::Base04.rems(cx))
        .px(DynamicSpacing::Base04.rems(cx))
        .py(DynamicSpacing::Base02.rems(cx))
        .border_b_1()
        .border_color(cx.theme().colors().border_variant)
        .children(["Name", "Value", "Path", "Expires"].map(|heading| {
            div().flex_1().min_w_0().child(
                Label::new(heading)
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
        }))
        .child(div().flex_none().w(px_to_rems(ACTION_COLUMN_PX)))
}

/// The state where there is nothing to list: a jar no response has put a cookie in yet.
fn empty_state(cx: &App) -> impl IntoElement {
    v_flex()
        .w_full()
        .items_center()
        .justify_center()
        .py(DynamicSpacing::Base12.rems(cx))
        .gap(DynamicSpacing::Base04.rems(cx))
        .child(Headline::new("No cookies stored").size(HeadlineSize::Small))
        .child(Label::new("Cookies a response sets are listed here.").color(Color::Muted))
}
