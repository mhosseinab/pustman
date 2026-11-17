# Pustman design system (`crates/ui` + `crates/theme`)

Modeled on Zed's `ui`/`theme` crates (GPL — we reimplement, we do not copy). The point of the layers:
features never pick raw colors, pixel sizes, or ad-hoc spacing. They pick *intent*.

```
theme tokens (ThemeColors, StatusColors)  ← JSON theme files, light + dark
        ↓
semantic styles (Color, TextSize, DynamicSpacing, ElevationIndex, UiDensity)
        ↓
components (Button, Label, ListItem, Tab, Table, Modal, …)   ← crates/ui
        ↓
feature views (RequestEditor, ResponseViewer, …)            ← only compose components
```

## 1. Theme tokens

Access: `cx.theme().colors().<token>` and `cx.theme().status().<token>` via an `ActiveTheme` trait on `App`.
Token families (adopt Zed's names so Zed themes can be ported):

| Family | Tokens | Used for |
| --- | --- | --- |
| Backgrounds | `background`, `surface_background`, `elevated_surface_background`, `panel_background`, `editor_background` | Layers of the window |
| Borders | `border`, `border_variant`, `border_focused`, `border_selected`, `border_transparent`, `border_disabled` | `border` = strong dividers; `border_variant` = subtle internal dividers |
| Elements | `element_background`, `element_hover`, `element_active`, `element_selected`, `element_disabled` | Filled controls (inputs, filled buttons) |
| Ghost elements | `ghost_element_background`/`_hover`/`_active`/`_selected`/`_disabled` | Transparent controls (list rows, subtle buttons, tabs) |
| Text | `text`, `text_muted`, `text_placeholder`, `text_disabled`, `text_accent` | |
| Icons | `icon`, `icon_muted`, `icon_disabled`, `icon_accent` | |
| Chrome | `title_bar_background`, `status_bar_background`, `toolbar_background`, `tab_bar_background`, `tab_active_background`, `tab_inactive_background` | |
| Panes | `pane_focused_border`, `pane_group_border`, `panel_focused_border` | Focus indication on containers |
| Scrollbar | `scrollbar_thumb_background`, `_hover_background`, `scrollbar_track_background` | |
| DnD | `drop_target_background`, `drop_target_border` | |
| Search | `search_match_background` | Find-in-response highlights |
| Links | `link_text_hover` | |

Status colors (`cx.theme().status()`), each with `_background` and `_border` variants:
`error, warning, info, success, hint, created, deleted, modified, conflict, hidden, ignored, predictive, renamed, unreachable`.

**Pustman additions** (add to our `ThemeColors`, define in every theme):
- HTTP methods: `method_get`, `method_post`, `method_put`, `method_patch`, `method_delete`, `method_head`, `method_options` — keep Postman's familiar hue mapping (GET green, POST amber, PUT blue, PATCH purple, DELETE red) but tuned per theme for contrast.
- Syntax for response bodies: `syntax_key`, `syntax_string`, `syntax_number`, `syntax_boolean`, `syntax_null`, `syntax_punctuation`.
- `variable_resolved`, `variable_unresolved` for `{{var}}` highlighting in inputs.

## 2. Semantic `Color`

Labels and icons take `Color`, never `Hsla`:

`Default, Muted, Hint, Placeholder, Disabled, Accent, Selected, Hidden, Error, Warning, Info, Success, Created, Modified, Deleted, Conflict, Ignored, Player(u32), Custom(Hsla)`.

`Color::Custom` is an escape hatch that needs a reviewer-justified reason. `Color::color(cx) -> Hsla`
resolves against the active theme.

## 3. Typography

All sizes in **rems** so they follow `ui_font_size` (user setting) and HiDPI.

| `TextSize` | Default px | Use |
| --- | --- | --- |
| `Large` | 16 | Modal titles, empty-state headlines |
| `Default` | 14 | Body UI text |
| `Small` | 12 | Secondary labels, table cells, tab titles, status bar |
| `XSmall` | 10 | Badges, counts, keybinding hints |
| `Ui` | ui_font_size setting | Matches user preference |
| `Editor` | buffer_font_size setting | URL bar, body editor, response body (monospace) |

Helpers via a `StyledTypography` trait: `text_ui_lg`, `text_ui`, `text_ui_sm`, `text_ui_xs`, `text_buffer`,
`font_ui`, `font_buffer`. `Headline`/`HeadlineSize` for headings, `Label`/`LabelSize` for everything else.

Rules: one font family for UI, one monospace for content (URLs, headers values, bodies). Never bold to
create hierarchy when size/color can. Use tabular numbers for times and sizes so columns don't jitter.

## 4. Spacing and density

`UiDensity` setting: `Compact` (×0.75), `Default` (×1.0), `Comfortable` (×1.25).
`DynamicSpacing::BaseNN.rems(cx)` / `.px(cx)` resolves per density (values in px, Compact/Default/Comfortable):

| Token | C | D | C+ |
| --- | --- | --- | --- |
| Base00 | 0 | 0 | 0 |
| Base01 | 1 | 1 | 2 |
| Base02 | 1 | 2 | 4 |
| Base03 | 2 | 3 | 4 |
| Base04 | 2 | 4 | 6 |
| Base06 | 3 | 6 | 8 |
| Base08 | 4 | 8 | 10 |
| Base12 | 10 | 12 | 14 |
| Base16 | 14 | 16 | 18 |
| Base20 | 18 | 20 | 22 |
| Base24/32/40/48 | fixed | | |

Use the Tailwind spacing methods (`gap_1`, `p_2`) only for fixed geometry that must not change with
density (icon optical alignment, borders).

## 5. Elevation

`ElevationIndex`: `Background` < `Surface` < `EditorSurface` < `ElevatedSurface` < `ModalSurface`.
Each level defines its background, shadow, and `on_elevation_bg` (the background for things sitting on it).

`StyledExt` helpers:
- `elevation_1(cx)` — surfaces like dropdown triggers that float slightly.
- `elevation_2(cx)` — popovers, menus, pickers, tooltips: `elevated_surface_background`, `rounded_lg`, `border_1`, `border_variant`, shadow.
- `elevation_3(cx)` — modals.

Never stack shadows manually; pick the elevation that matches the layer.

## 6. Icons

- 16×16 viewBox, 1.2px stroke, content in a 12×12 inner box, derived from Lucide (ISC) — record the source.
- Files `assets/icons/snake_case.svg`, enum `IconName::PascalCase`, `IconSize::{XSmall, Small, Medium, …}`.
- Icons take `Color` (`Icon::new(IconName::Send).color(Color::Muted)`).
- Icon-only buttons always have a tooltip and an accessible label.
- Need a new icon? Ask a designer/user for the SVG; do not hand-draw paths.

## 7. Theming rules

- Ship one light and one dark theme from day one; follow the OS appearance by default (`window.appearance()` + observer).
- Every new token must be defined in every bundled theme; add a test that deserializes all theme files
  and asserts no missing tokens.
- Contrast: body text ≥ 4.5:1, large text/icons ≥ 3:1 against their actual background in both themes.
- Never convey state by color alone (method badges also show text; status pill shows the code).
