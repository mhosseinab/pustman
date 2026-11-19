---
name: gpui-testing
description: Write deterministic GPUI tests for Pustman views, actions, async tasks and HTTP flows using the gpui::test attribute, TestAppContext and VisualTestContext, including keystroke simulation, clock control, randomized scheduling and fake HTTP. Use when writing or fixing tests for any crate that uses gpui, when a test hangs or panics with "Parking forbidden.", or when testing async request sending, debouncing, keybindings or rendering behavior.
---

# GPUI testing

Verified against Zed `0cc1ffb6` (`crates/gpui/src/app/test_context.rs`, `src/executor.rs`,
`examples/testing.rs`). Write the failing test first. Look up APIs in gpui.rs and the pinned source
first; never Context7/docs.rs for GPUI (see `rust-gpui-ui-ux` "Framework docs first").

## Basics

```rust
#[gpui::test]
async fn sends_request_on_ctrl_enter(cx: &mut TestAppContext) {
    init_test(cx);
    let transport = FakeTransport::with_response(200, r#"{"ok":true}"#);
    let (editor, cx) = cx.add_window_view(|_window, cx| RequestEditor::new(transport.clone(), cx));

    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_input("https://example.test/users");
    cx.simulate_keystrokes("ctrl-enter");
    cx.run_until_parked();                                 // drain all tasks deterministically

    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.last_status(), Some(200));
    });
    assert_eq!(transport.requests().len(), 1);
}

fn init_test(cx: &mut TestAppContext) {
    cx.update(|cx| {
        settings::init_test(cx);       // our crates expose test-support init helpers
        theme::init_test(cx);
        request_editor::init(cx);
        cx.bind_keys([KeyBinding::new("ctrl-enter", SendRequest, Some("RequestEditor"))]);
    });
}
```

- Test fn params can be any of: `&mut TestAppContext` (one or several for multi-app tests), `BackgroundExecutor`, `StdRng`.
- `#[gpui::test(iterations = 50)]` reruns with different scheduler seeds; `#[gpui::test(seed = 7)]` pins one.
  Env vars: `SEED`, `ITERATIONS`, `PENDING_TRACES=1` (prints where pending tasks were spawned).
- `cx.add_window_view(|window, cx| View::new(...))` → `(Entity<View>, &mut VisualTestContext)`;
  `cx.add_empty_window()` for window-only needs.
- `VisualTestContext`: `simulate_keystrokes("ctrl-a ctrl-c")`, `simulate_input("text")`, `dispatch_action(Action)`,
  `simulate_click(point, modifiers)`, `run_until_parked()`, `debug_bounds("selector")` (pair with `.debug_selector(|| "x".into())` on elements).
- Time: `cx.executor().advance_clock(Duration::from_millis(300))` for debounces/timeouts; never real sleeps.
  Inside code under test, timers must be `cx.background_executor().timer(d)` so the fake clock drives them.
- Prompts/dialogs: `simulate_prompt_answer("Don't Save")`, `has_pending_prompt()`, `simulate_new_path_selection(...)`.
- Clipboard: `read_from_clipboard()`.

## Async / HTTP

- `http_engine` depends on an `HttpTransport` trait object; tests inject a `FakeTransport` (records requests, returns scripted
  responses, can delay via the GPUI timer or fail with DNS/TLS/timeout errors). No real network in unit tests.
- Test cancellation: start send, dispatch `CancelRequest`, `run_until_parked`, assert the fake saw a dropped future
  and the view shows "Cancelled".
- Tokio-backed code: integration tests that truly need tokio initialize `gpui_tokio::init(cx)`; keep them few.

## Failure triage

| Symptom | Cause | Fix |
| --- | --- | --- |
| "Parking forbidden." panic | A task is awaited that nothing will ever wake (real IO, real timer, dropped sender) | Use fakes and GPUI timers; or the task was dropped/never detached |
| Test passes alone, fails with `iterations` | Order-dependent async logic | Fix the race; don't remove iterations |
| Action does nothing | Missing `bind_keys`, missing `init`, or view not focused | Bind in `init_test`; focus via `cx.update(\|window, cx\| window.focus(&handle, cx))` |
| Re-entrant update panic | Updating an entity inside its own update | Emit an event or defer with `cx.defer` |

## Conventions
- Crates expose `test-support` features with fakes and `init_test` helpers; dev-dependencies enable them:
  `request_editor = { workspace = true, features = ["test-support"] }`.
- Test names describe behavior (`cancelling_send_restores_send_button`), one behavior per test.
- Don't weaken assertions to make flaky tests pass; find the race.
