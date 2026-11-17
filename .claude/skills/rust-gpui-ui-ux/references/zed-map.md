# Where to look in Zed

Paths are relative to a Zed checkout at `58849026ec30fb225dc5d07cccf37e8408b85555`.
License column: **A** = Apache-2.0 (may depend on / adapt), **G** = GPL-3.0 (study only, reimplement).

| Pattern | Zed path | Lic. | Pustman analogue |
| --- | --- | --- | --- |
| Ownership, entities, contexts | `crates/gpui/src/_ownership_and_data_flow.rs` | A | all views |
| Accessibility guide | `crates/gpui/src/_accessibility.rs`, `crates/gpui/examples/a11y.rs` | A | all components |
| Minimal app | `crates/gpui/examples/hello_world.rs` | A | `crates/pustman/src/main.rs` |
| Text input from scratch | `crates/gpui/examples/input.rs` | A | `crates/text_input` |
| Virtualized lists | `crates/gpui/examples/uniform_list.rs`, `list_example.rs`, `src/elements/{uniform_list,list}.rs` | A | tree, history, console |
| Drag & drop | `crates/gpui/examples/drag_drop.rs` | A | tree reorder, tabs |
| Tab order / focus-visible | `crates/gpui/examples/{tab_stop,focus_visible}.rs`, `src/tab_stop.rs` | A | forms, tables |
| Popovers / anchored | `crates/gpui/examples/popover.rs`, `src/elements/{anchored,deferred}.rs` | A | menus |
| Data table example | `crates/gpui/examples/data_table.rs` | A | headers/params tables |
| GPUI tests | `crates/gpui/examples/testing.rs`, `src/test.rs` | A | all tests |
| Tokio bridge | `crates/gpui_tokio/src/gpui_tokio.rs` | A | `http_engine` |
| HTTP client trait | `crates/http_client/src/http_client.rs`, `crates/reqwest_client` | A | `http_engine` |
| Design tokens: spacing, typography, elevation, color | `crates/ui/src/styles/*.rs` | G | `crates/ui/src/styles/` |
| Components | `crates/ui/src/components/**` (button, tab, tab_bar, list, context_menu, popover_menu, tooltip, data_table, modal, scrollbar) | G | `crates/ui/src/components/` |
| Component previews | `crates/component/src/component.rs` | G | component gallery |
| Theme colors & status | `crates/theme/src/styles/{colors,status}.rs`, `ui_density.rs` | G | `crates/theme` |
| Workspace, panes, items, docks | `crates/workspace/src/{workspace,pane,item,dock,modal_layer,notifications,status_bar}.rs` | G | `crates/workspace` |
| Picker | `crates/picker/src/picker.rs` | G | env picker, command palette |
| Small modal feature end-to-end | `crates/go_to_line/` | G | `environment_picker` shape |
| Tree panel | `crates/project_panel/` | G | `collections_panel` |
| Settings store & macros | `crates/settings/` , `assets/settings/default.json` | G | `crates/settings` |
| SQLite persistence | `crates/db/`, `crates/sqlez/` | G | `crates/storage` |
| Keymaps | `assets/keymaps/default-{macos,linux,windows}.json` | G (data) | `assets/keymaps/` |
| UI checklist / feature process | `docs/src/development/{ui-checklist,feature-process,glossary}.md` | — | `ux-checklist.md` |
| Icon guidelines | `crates/icons/README.md` | G | `design-system.md#6-icons` |
| Lints | `tooling/lints/README.md`, `clippy.toml`, root `Cargo.toml [workspace.lints]` | — | `pustman-feature-crate` skill |
