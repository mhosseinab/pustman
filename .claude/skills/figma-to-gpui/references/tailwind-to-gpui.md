# Tailwind (Figma reference) → GPUI

Method names verified against `crates/gpui/src/styled.rs` and `crates/gpui_macros/src/styles.rs` at Zed
`58849026`. Tailwind fractions/decimals become `_` and `p`: `w-1/2` → `w_1_2()`, `gap-1.5` → `gap_1p5()`.
Generated suffixes on spacing/size methods: `0 0p5 1 1p5 2 2p5 3 3p5 4 5 6 7 8 9 10 11 12 16 20 24 32 40 48 56 64 72 80 96 112 128`,
pixel forms (`1px`…`32px`), fractions, `full`, `auto` (where allowed). Anything else: `.w(px(…))`, `.p(rems(…))`.

| Tailwind | GPUI | Note |
| --- | --- | --- |
| `flex flex-row` / `flex flex-col` | `h_flex()` / `v_flex()` (our `ui` helpers) or `div().flex().flex_row()` / `.flex_col()` | |
| `items-center justify-between` | `.items_center().justify_between()` | also `items_start/end`, `justify_center/end` |
| `flex-1` `grow` `shrink-0` `flex-wrap` | `.flex_1()` `.flex_grow()` `.flex_shrink_0()` `.flex_wrap()` | |
| `gap-2` `gap-x-1` | `.gap_2()` `.gap_x_1()` | prefer the `ui` spacing helper when density should apply |
| `p-2 px-3 pt-1` `m-*` | `.p_2().px_3().pt_1()` `.m_*()` | |
| `w-full h-full` `size-4` | `.w_full().h_full()` `.size_4()` | `size_full()` for both |
| `min-w-0` `max-w-md` | `.min_w_0()` `.max_w_*()` | `min_w_0` is required for truncation in flex rows |
| `absolute relative inset-0 top-0` | `.absolute() .relative() .inset_0() .top_0()` | prefer flex; overlays use `deferred(anchored())` |
| `overflow-hidden` | `.overflow_hidden()` | scrolling needs `.id(…)` + `.overflow_y_scroll()` |
| `rounded rounded-md rounded-full rounded-t-lg` | `.rounded_*()` (`rounded_sm/md/lg/full`, side variants `rounded_t_*`, `rounded_tl_*`) | |
| `border border-b border-dashed` | `.border_1() .border_b_1() .border_dashed()` | |
| `border-[color]` | `.border_color(cx.theme().colors().border)` | always a token |
| `bg-[color]` | `.bg(cx.theme().colors().<token>)` | always a token |
| `text-[color]` | `.text_color(...)` / `Label::new(..).color(Color::Muted)` | prefer semantic `Color` |
| `text-xs/sm/base/lg/xl/2xl` | `.text_xs() … .text_2xl()` | UI text: `Label` + `LabelSize` from `ui` |
| `font-medium/semibold/bold` | `.font_weight(FontWeight::MEDIUM/SEMIBOLD/BOLD)` | |
| `font-mono` | `.font_family(<buffer font from settings>)` | never hard-code a family |
| `leading-*` | `.line_height(...)` | |
| `truncate` `line-clamp-2` `whitespace-nowrap` | `.truncate()` `.line_clamp(2)` `.whitespace_nowrap()` | `.text_ellipsis()` is also available |
| `text-center` `italic` `underline` | `.text_center()` `.italic()` `.underline()` | |
| `shadow-sm/md/lg` | `.shadow_sm()/_md()/_lg()` | elevated surfaces use our elevation helper instead |
| `opacity-50` | `.opacity(0.5)` | disabled state uses `Color::Disabled`, not opacity |
| `cursor-pointer` | `.cursor_pointer()` | needs `.id(…)` and a keyboard path too |
| `hover:bg-*` `active:*` | `.hover(\|style\| style.bg(...))` `.active(\|style\| …)` | requires `.id(…)` for `active` |
| `focus-visible:ring` | `.focus_visible(\|style\| style.border_color(cx.theme().colors().border_focused))` | |
| `<img src>` (raster / multi-color) | `img("images/…")` | |
| `<svg>` single-color icon | `svg().path("icons/x.svg").size_4().text_color(...)` or our `Icon` component | color comes from `text_color` |

Divs from the reference that only carry Figma frame names (`data-name`, wrapper divs with no style) are dropped.
