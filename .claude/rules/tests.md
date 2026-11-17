---
paths:
  - "**/tests/**/*.rs"
  - "**/*_tests.rs"
  - "**/test_support.rs"
---

# Test traps

Load the `gpui-testing` skill for GPUI tests.

- `std::thread::sleep`, `smol::Timer`, and real network/filesystem make GPUI tests hang (panic: "Parking forbidden.") or flake. Use `executor.timer`, `advance_clock`, `FakeTransport`, and temp dirs via a fake fs or `tempfile`.
- Forgetting `run_until_parked()` after an action that spawns work → assertions run before the task completes.
- Don't delete `iterations = N` to make a test pass; a failure under random scheduling is a real race.
- Fixtures containing secrets must be obviously fake (`token-REDACTED-example`) so secret scanners and reviewers don't trip.
