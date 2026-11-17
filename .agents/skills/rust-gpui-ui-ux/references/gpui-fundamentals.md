# GPUI fundamentals

Verified against Zed `58849026`. All snippets are original Pustman-style examples; they are shape
references and were not compiled in isolation — adapt imports to our `ui::prelude::*`.

## Contexts

| Type | Where you get it | Use |
| --- | --- | --- |
| `&mut App` | `init`, globals, `RenderOnce::render`, element listeners | App-wide state, globals, open windows, create entities |
| `&mut Context<T>` | `Render::render`, `cx.listener` handlers, `entity.update` | Everything `App` can do (it derefs) + `notify`, `emit`, `listener`, `processor`, `spawn` bound to `T` |
| `&mut Window` | passed *before* `cx` in render and handlers | Focus, text system, bounds, `window.focus(&handle, cx)`, `window.dispatch_action` |
| `AsyncApp` / `AsyncWindowContext` | inside `cx.spawn` | Hold across `.await`; every access is fallible because the app/window may be gone |

Parameter order is always `(…, window: &mut Window, cx: &mut Context<Self>)`.

## Entities

```rust
let editor: Entity<RequestEditor> = cx.new(|cx| RequestEditor::new(request_id, cx));

editor.read(cx).url();                                   // borrow immutably
editor.update(cx, |editor, cx| {                         // use the INNER cx
    editor.set_method(Method::Post, cx);
});
let weak: WeakEntity<RequestEditor> = editor.downgrade(); // for back-pointers / async
weak.update(cx, |editor, cx| editor.mark_sent(cx))?;     // fails if dropped -> Result
```

- Re-entrant `update` on the same entity panics. If a child needs the parent, emit an event instead.
- Entities are reference-counted handles: cloning is cheap; keep strong handles only where you own the thing.
- Back-references (child → parent, item → workspace) are `WeakEntity`.

## Views vs components

| | View (`impl Render`) | Component (`#[derive(IntoElement)] + impl RenderOnce`) |
| --- | --- | --- |
| Has state across frames | yes, lives in an `Entity` | no, rebuilt each frame by its parent |
| Receives focus/actions | yes (`FocusHandle`, `key_context`) | only via callbacks passed in |
| Examples | `RequestEditor`, `ResponseViewer`, `CollectionsPanel`, `EnvironmentPicker` | `Button`, `MethodBadge`, `StatusPill`, `KeyValueRow`, `EmptyState` |

```rust
// Stateful view
pub struct ResponseViewer {
    focus_handle: FocusHandle,
    response: Option<Arc<HttpResponse>>,
    active_tab: ResponseTab,
    _subscriptions: Vec<Subscription>,
}

impl Render for ResponseViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("ResponseViewer")
            .track_focus(&self.focus_handle)
            .size_full()
            .child(self.render_status_row(cx))
            .child(self.render_body(cx))
    }
}

// Stateless component with a builder API
#[derive(IntoElement)]
pub struct StatusPill {
    status: u16,
}

impl StatusPill {
    pub fn new(status: u16) -> Self { Self { status } }
}

impl RenderOnce for StatusPill {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = match self.status {
            200..=299 => Color::Success,
            300..=399 => Color::Info,
            400..=499 => Color::Warning,
            _ => Color::Error,
        };
        h_flex()
            .px_1p5()
            .rounded_sm()
            .bg(color.color(cx).opacity(0.12))
            .child(Label::new(format!("{} {}", self.status, reason_phrase(self.status)))
                .size(LabelSize::Small)
                .color(color))
    }
}
```

Rule of thumb: start with a component; promote to a view only when it needs focus, its own async work,
or state that survives re-render (scroll position, text selection, expansion).

## Events, observation, globals

```rust
pub enum RequestEditorEvent { Sent { request_id: RequestId }, DirtyChanged }
impl EventEmitter<RequestEditorEvent> for RequestEditor {}

// emitter
cx.emit(RequestEditorEvent::DirtyChanged);

// listener (store the subscription!)
self._subscriptions.push(cx.subscribe_in(&editor, window, |this, _editor, event, window, cx| {
    match event {
        RequestEditorEvent::Sent { request_id } => this.history.record(*request_id, cx),
        RequestEditorEvent::DirtyChanged => cx.notify(),
    }
}));

// re-render when another entity notifies
self._subscriptions.push(cx.observe(&environment_store, |_, _, cx| cx.notify()));

// react to settings/theme globals
self._subscriptions.push(cx.observe_global::<SettingsStore>(|this, cx| { this.reload_settings(cx); cx.notify(); }));

// run setup for every new instance of a type (how crates register actions on Workspace)
cx.observe_new(|workspace: &mut Workspace, _window, _cx| { workspace.register_action(…); }).detach();
```

Use events for "something happened", observation for "something changed, re-read it". Globals
(`impl Global for X`, `cx.set_global`, `cx.global::<X>()`) for app-wide singletons like `SettingsStore`
or the active theme registry.

## Tasks and async

```rust
pub struct RequestEditor {
    pending_send: Option<Task<()>>, // replacing it cancels the in-flight request
    …
}

fn send(&mut self, _: &SendRequest, _window: &mut Window, cx: &mut Context<Self>) {
    let request = self.build_request(cx);
    let engine = self.engine.clone();
    self.state = SendState::Sending { started: Instant::now() };
    cx.notify();

    self.pending_send = Some(cx.spawn(async move |this, cx| {
        let result = engine.send(request).await;              // runs on tokio via gpui_tokio inside the engine
        this.update(cx, |this, cx| {
            this.state = match result {
                Ok(response) => SendState::Done(Arc::new(response)),
                Err(error) => SendState::Failed(error.to_string().into()),
            };
            this.pending_send = None;
            cx.notify();
        })
        .ok();                                                 // view closed: nothing to do
    }));
}

fn cancel_send(&mut self, _: &CancelRequest, _: &mut Window, cx: &mut Context<Self>) {
    self.pending_send = None;                                  // drop == cancel
    self.state = SendState::Cancelled;
    cx.notify();
}
```

- `cx.spawn` runs on the foreground (UI) thread between awaits; never do CPU-heavy or blocking work in it.
- `cx.background_spawn(async move { … })` for CPU work (pretty-printing a 5 MB JSON body, parsing a collection).
- `gpui_tokio::Tokio::spawn(cx, fut)` for tokio-only libraries (reqwest). Call `gpui_tokio::init(cx)` in `main`.
- `.detach_and_log_err(cx)` for fire-and-forget fallible work; prefer surfacing errors to the user.
- `Task::ready(value)` when an API returns a task but the answer is already known.

## Elements and styling

GPUI styling mirrors Tailwind: `div().flex().flex_col().gap_2().p_2().rounded_md().border_1()`.
Our `ui` prelude adds `h_flex()` (row, items centered) and `v_flex()` (column).

| Category | Methods |
| --- | --- |
| Layout | `flex`, `flex_col`, `flex_1`, `flex_none`, `flex_grow`, `flex_shrink_0`, `items_center`, `justify_between`, `gap_*`, `grid`, `size_full`, `w_full`, `h(px/rems)`, `min_w_0`, `max_w` |
| Spacing | `p_*`, `px_*`, `py_*`, `m_*` (prefer `DynamicSpacing::BaseNN.rems(cx)` for density-aware) |
| Visual | `bg`, `border_1`, `border_color`, `rounded_sm/md/lg`, `shadow_*`, `opacity` |
| Text | `text_color`, `text_size`, `font_weight`, `truncate`, `text_ellipsis`, `text_ellipsis_middle` (URLs!), `line_clamp(n)`, `whitespace_nowrap` |
| Overflow | `overflow_hidden`, `overflow_y_scroll` (needs `.id()`), `track_scroll(&ScrollHandle)` |
| State | `hover(|s| …)`, `active(|s| …)`, `focus_visible(|s| …)`, `group("row")` + `group_hover("row", |s| …)` |
| Conditionals | `.when(cond, |el| …)`, `.when_some(option, |el, value| …)`, `.map(|el| …)` |

```rust
h_flex()
    .id(("header-row", row.id))              // stateful: needed for click/hover/tooltip/scroll
    .group("header-row")
    .w_full()
    .min_w_0()
    .gap(DynamicSpacing::Base04.rems(cx))
    .px(DynamicSpacing::Base06.rems(cx))
    .hover(|style| style.bg(cx.theme().colors().ghost_element_hover))
    .when(row.disabled, |row_el| row_el.opacity(0.5))
    .child(Checkbox::new(("header-enabled", row.id), row.enabled.into()))
    .child(div().flex_1().min_w_0().truncate().child(row.key.clone()))
    .child(
        IconButton::new(("remove-header", row.id), IconName::Trash)
            .visible_on_hover("header-row")   // revealed by group hover
            .on_click(cx.listener(move |this, _, _, cx| this.remove_header(row_id, cx))),
    )
```

`.id(...)` turns a `Div` into a `Stateful<Div>`: required for click, hover state, tooltips, scrolling,
drag. IDs accept `&'static str`, `SharedString`, integers and tuples like `("name", u64)`.

## Lists (virtualization)

```rust
// Uniform row height: tree rows, history, header tables, response lines.
uniform_list(
    "history-list",
    self.entries.len(),
    cx.processor(|this, range: Range<usize>, _window, cx| {
        range.map(|ix| this.render_entry(ix, cx)).collect::<Vec<_>>()
    }),
)
.track_scroll(&self.scroll_handle)           // UniformListScrollHandle, keep it in the view
.size_full()

// Selection follows keyboard:
self.scroll_handle.scroll_to_item(self.selected_index, ScrollStrategy::Top);
```

```rust
// Variable heights: console log, websocket messages. ListState lives in the view.
self.list_state = ListState::new(messages.len(), ListAlignment::Bottom, px(512.));
list(self.list_state.clone(), cx.processor(|this, ix: usize, _window, cx| this.render_message(ix, cx)))
    .size_full()
// On data change: self.list_state.splice(old_range, new_count);
```

## Overlays, popovers, drag & drop, custom paint

- **Popover/menus:** render inside `deferred(anchored().child(menu))` so they paint above siblings; use our
  `PopoverMenu`/`ContextMenu` components rather than hand-building. Dismiss on `on_mouse_down_out` and `menu::Cancel`.
- **Drag & drop** (reorder requests in the tree, drag a request into a tab bar):
  ```rust
  .on_drag(DraggedRequest { id }, |dragged, _offset, _window, cx| cx.new(|_| DragPreview::new(dragged.id)))
  .drag_over::<DraggedRequest>(|style, _, _, cx| style.bg(cx.theme().colors().drop_target_background))
  .on_drop(cx.listener(|this, dragged: &DraggedRequest, _window, cx| this.move_request(dragged.id, cx)))
  ```
- **Custom paint** (response-time waterfall, size sparkline): `canvas(prepaint, paint)`; keep it cheap.
- **Animation:** `.with_animation(id, Animation::new(Duration::from_millis(150)).with_easing(ease_in_out), |el, delta| el.opacity(delta))`. Animate only to communicate state change; respect a reduce-motion setting.

## App bootstrap (`crates/pustman/src/main.rs` shape)

```rust
fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        gpui_tokio::init(cx);
        settings::init(cx);
        theme::init(cx);
        ui::init(cx);
        workspace::init(cx);
        collections_panel::init(cx);
        request_editor::init(cx);
        response_viewer::init(cx);
        environment_picker::init(cx);
        command_palette::init(cx);
        cx.bind_keys(keymap::load_default(cx));   // from assets/keymaps/default-<os>.json

        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            |window, cx| cx.new(|cx| Workspace::new(window, cx)),
        )
        .log_err();                                 // never unwrap in main either: show a native dialog on failure
        cx.activate(true);
    });
}
```
