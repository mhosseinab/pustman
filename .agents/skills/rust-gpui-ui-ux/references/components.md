# Components (`crates/ui`)

Our component library mirrors the *API shape* of Zed's `ui` crate so Zed code reads as a guide, but it
is our own implementation. Import everything through `use ui::prelude::*;`.

## Authoring rules

1. `#[derive(IntoElement)]` + `impl RenderOnce`. Constructor takes the required data (`new(id, label)`);
   everything else is a chainable builder method that consumes and returns `Self`.
2. Interactive components take an `id: impl Into<ElementId>` as the first argument.
3. Implement the shared behavior traits instead of ad-hoc methods:
   `Clickable` (`on_click`, `cursor_style`), `Toggleable` (`toggle_state(impl Into<ToggleState>)`),
   `Disableable` (`disabled(bool)`), `FixedWidth` (`width`, `full_width`), `VisibleOnHover` (`visible_on_hover(group)`).
4. Callbacks are `Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>` stored in an `Option`.
5. Disabled state: no handler fires, color becomes `Color::Disabled`, cursor not-allowed, still focusable for screen readers when meaningful.
6. Set role + accessible label inside the component so callers get a11y for free (Button sets `Role::Button` and uses its label as `aria_label`).
7. Every component gets a preview (see "Gallery") listing all variants and states.

## Catalogue

| Component | Key API | Pustman uses |
| --- | --- | --- |
| `Button` | `Button::new(id, label).style(ButtonStyle::Filled).size(ButtonSize::Medium).start_icon(Icon)`, `.loading(bool)`, `.key_binding(KeyBinding)`, `.tooltip(...)`, `.full_width()` | **Send** (Filled, accent), Save, Import |
| `IconButton` | `IconButton::new(id, IconName::X).icon_size(..).shape(Square)` + mandatory tooltip | Close tab, add row, copy response |
| `ButtonStyle` | `Filled`, `Tinted(TintColor::{Accent,Error,Warning,Success})`, `Outlined`, `OutlinedGhost`, `Subtle` (default), `Transparent` | One Filled primary per region |
| `ButtonSize` | `Large` 32px, `Medium` 28px, `Default` 22px, `Compact` 18px, `None` | URL bar row uses Medium |
| `Label` / `HighlightedLabel` / `LoadingLabel` | `.size(LabelSize)`, `.color(Color)`, `.truncate()`, highlight indices for fuzzy matches | Everywhere; Highlighted in pickers |
| `Headline` | `Headline::new(text).size(HeadlineSize::Small)` | Empty states, modal titles |
| `EmptyState` | `EmptyState::new(id, icon, title).description(text).child(action)` — icon at `IconSize::Large`, title at `HeadlineSize::Large`, copy at `LabelSize::Large` | History and Welcome empty states |
| `Icon` | `Icon::new(IconName).size(IconSize).color(Color)` | |
| `Checkbox` / `Switch` (`Toggle`) | `ToggleState::{Selected, Unselected, Indeterminate}` | Enable/disable header/param rows; "select all" uses Indeterminate |
| `Tab` / `TabBar` | `Tab::new(id).position(TabPosition).close_side(..).start_slot(..).end_slot(..).toggle_state(active)`; `TabBar::new(id).start_child(..).children(tabs).end_child(..)` | Request tabs, Params/Headers/Body tabs |
| `ListItem` / `ListHeader` / `ListSubHeader` | `ListItem::new(id).inset(true).indent_level(n).toggle(Some(expanded)).start_slot(icon).end_slot(..).toggle_state(selected).on_click(..).on_secondary_mouse_down(..)` | Collections tree, history |
| `Disclosure` | chevron toggle, `Disclosure::new(id, is_open).on_toggle(..)` | Folder expand, "Show more headers" |
| `Divider` | `Divider::horizontal()`, `.vertical()`, `.color(DividerColor::BorderVariant)` | |
| `Indicator` | dot/bar status indicator | Dirty tab, unsaved changes, connection state |
| `Chip` | small labeled tag | Content-Type, environment name |
| `KeyBinding` | renders the binding for an action from the active keymap (`.size(TextSize)` where hints read at body size) | Tooltips, menus, empty states |
| `Tooltip` | `Tooltip::text("Send")`, `Tooltip::for_action("Send request", &SendRequest, cx)`, `Tooltip::with_meta(title, action, meta, cx)` | Every icon button; every truncated text |
| `ContextMenu` | `ContextMenu::build(window, cx, \|menu, _, _\| menu.header("Request").entry("Duplicate", Some(Box::new(Duplicate)), handler).separator().toggleable_entry(..).action("Delete", Box::new(Delete)))` | Right-click on tree items, tabs, rows |
| `PopoverMenu` | `PopoverMenu::new(id).trigger(button).menu(move \|window, cx\| Some(menu)).anchor(Corner::TopLeft).with_handle(handle)` | Method dropdown, body type dropdown |
| `right_click_menu` | `right_click_menu(id).trigger(\|..\| el).menu(\|window, cx\| menu)` | Tree rows |
| `DropdownMenu` | label + popover in one | Auth type, body mode |
| `Modal` (+ `ModalHeader`, `ModalFooter`, `Section`) | Content of a `ModalView` | Import collection, manage environments |
| `Popover` | elevated container for custom popovers | Variable hover card |
| `Banner` / `Callout` | inline, contextual message with severity + actions | "Variable `{{token}}` is unresolved", TLS disabled warning |
| `Notification` / Toast | via workspace `show_toast`/`show_error` | Copied, saved, import failed |
| `Table` (data table) | `Table::new(columns).striped().header(..).row(..)` or virtualized via `uniform_list`; resizable columns, `TableInteractionState` | Params, headers, form-data, cookies, response headers |
| `Scrollbar` | wraps a `ScrollHandle`; show-on-scroll setting | Long panes |
| `Progress` / spinner | `IconName::LoadCircle` rotating, or progress bar | Sending, importing |
| `TreeViewItem` | indented row w/ disclosure | Alternative for collections tree |

## Example: Pustman's method selector (composition of primitives)

```rust
#[derive(IntoElement)]
pub struct MethodSelector {
    id: ElementId,
    method: Method,
    on_change: Option<Rc<dyn Fn(Method, &mut Window, &mut App)>>,
}

impl MethodSelector {
    pub fn new(id: impl Into<ElementId>, method: Method) -> Self {
        Self { id: id.into(), method, on_change: None }
    }

    pub fn on_change(mut self, handler: impl Fn(Method, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for MethodSelector {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let on_change = self.on_change;
        PopoverMenu::new(self.id.clone())
            .trigger(
                Button::new(self.id, self.method.as_str())
                    .style(ButtonStyle::Subtle)
                    .size(ButtonSize::Medium)
                    .color(self.method.color())      // Color::Custom-free: method colors are theme tokens
                    .end_icon(Icon::new(IconName::ChevronDown).size(IconSize::Small)),
            )
            .menu(move |window, cx| {
                let on_change = on_change.clone();
                Some(ContextMenu::build(window, cx, move |mut menu, _, _| {
                    for method in Method::ALL {
                        let on_change = on_change.clone();
                        menu = menu.entry(method.as_str(), None, move |window, cx| {
                            if let Some(on_change) = &on_change { on_change(method, window, cx) }
                        });
                    }
                    menu
                }))
            })
    }
}
```

(Exact builder names depend on our `ui` crate; keep them aligned with the table above.)

## Gallery (component previews)

Zed registers previews with a `Component` trait (`scope()`, `name()`, `description()`,
`preview(window, cx) -> Option<AnyElement>`) and a registration derive; previews are grouped
(`example_group_with_title`, `single_example`). Build the same for Pustman: a debug-only
"Component Gallery" item opened by `dev::OpenComponentGallery`. Each component preview shows every
style × size × state (default, hover via forced state, selected, disabled, loading) in both themes.
Reviewers look at the gallery, not at screenshots in PRs.
