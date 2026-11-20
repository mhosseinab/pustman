//! `integration_tests` — end-to-end proofs that cross crate boundaries (S37's redaction
//! NFR first): one scenario driven through the real `http_engine`, `storage`,
//! `workspace_store`, `console` and `secrets` code. This library target exists only so
//! the crate has a root; the proofs live in `tests/`.
