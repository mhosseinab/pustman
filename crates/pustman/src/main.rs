//! The window opener: `pustman` is the library holding the init order; this file runs the
//! app, calls [`pustman::init`] and opens the one workspace window (S22e). With
//! `PUSTMAN_STARTUP_TRACE=1` it prints `startup_ms=<n>` to stderr at the first painted
//! frame, so REQ-NFR-02's launch-to-interactive budget (docs/PRD.md) can be measured.

use std::time::Instant;

use gpui::{App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use workspace::Workspace;

fn main() {
    let started = Instant::now();
    let startup_trace = std::env::var("PUSTMAN_STARTUP_TRACE").is_ok_and(|value| value == "1");

    gpui_platform::application()
        .with_assets(ui::assets())
        .run(move |cx| {
            pustman::init(cx);
            if let Err(error) = open_workspace(cx, startup_trace, started) {
                // There is no window to show a toast on, so stderr is the only report left.
                eprintln!("pustman: could not open the workspace window: {error}");
                std::process::exit(1);
            }
        });
}

/// Open the workspace window and wire the shell to it: the problems the init order found
/// become error toasts on this window (AGENTS.md: errors reach the user), and closing it
/// quits the app — a windowed app has no meaning without its one window.
fn open_workspace(cx: &mut App, startup_trace: bool, started: Instant) -> anyhow::Result<()> {
    cx.on_window_closed(|cx, _window_id| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();

    let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
    let window = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Pustman".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let workspace = cx.new(|cx| Workspace::new(window, cx));
            if startup_trace {
                window.on_next_frame(move |_window, _cx| {
                    eprintln!("startup_ms={}", started.elapsed().as_millis());
                });
            }
            workspace
        },
    )?;

    let workspace = window.entity(cx)?;
    pustman::StartupProblems::attach_workspace(&workspace, cx);
    cx.activate(true);
    Ok(())
}
