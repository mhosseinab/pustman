---
name: pustman-http-engine
description: Build Pustman's request execution core — HTTP client abstraction over reqwest on gpui_tokio, cancellation, timing breakdown, redirects, TLS, proxies, cookies, body streaming, {{variable}} resolution with scopes, auth helpers, Postman collection v2.1 import/export, and fake HTTP for tests.
when_to_use: Use when working on sending requests, the http_engine/variables/api_model crates, environments and variable scopes, auth, cookies, import/export, response parsing, or any async networking code.
---

# HTTP engine

## Layering

```
request_editor (UI) ──build──▶ api_model::Request (unresolved, with {{vars}})
        │                              │
        │                     variables::resolve(scopes) ──▶ ResolvedRequest (no secrets logged)
        ▼                              ▼
  Task<…> held in view      http_engine::HttpEngine::send(ResolvedRequest) ──▶ HttpResponse + Timings
```

- `api_model`: plain serde types — `Request { method, url, params, headers, body, auth, settings }`, `Collection`,
  `Folder`, `Environment`, `Variable { key, value, secret, enabled }`. No GPUI, no IO. Postman v2.1 import/export lives here
  with round-trip tests against fixture files.
- `variables`: pure function `resolve(template, &ScopeStack) -> Resolved { text, unresolved: Vec<Name>, spans }`.
  Scope precedence (highest first): local/request → data → environment → collection → global. Return spans so the UI
  can highlight resolved/unresolved variables. Detect cycles. Dynamic variables (`{{$guid}}`, `{{$timestamp}}`) resolved at send time.
- `http_engine`: owns the client; no UI types.

## Client abstraction

Depend on a trait so tests use fakes (Zed's `http_client` crate is Apache-2.0 and shows the shape:
an `HttpClient` trait with `send`, `user_agent`, `proxy`, plus request-builder extensions for redirects/timeouts).

```rust
pub trait HttpTransport: Send + Sync + 'static {
    fn send(&self, request: ResolvedRequest) -> BoxFuture<'static, Result<HttpResponse, TransportError>>;
}

pub struct HttpEngine { transport: Arc<dyn HttpTransport> }

impl HttpEngine {
    /// Dropping the returned task drops the transport future, which cancels the request.
    pub fn send(&self, request: ResolvedRequest, cx: &App) -> Task<Result<HttpResponse, TransportError>> {
        let transport = self.transport.clone();
        cx.background_spawn(async move { transport.send(request).await })
    }
}
```

Only the production `ReqwestTransport` touches tokio (reqwest requires it): inside its `send` it runs the reqwest future
via `gpui_tokio::Tokio::spawn` (or a tokio handle captured at construction) and maps the `JoinError` to
`TransportError::Internal`. Keeping tokio inside that one impl lets `FakeTransport` tests run on GPUI's deterministic
executor without `gpui_tokio::init`. Production `main` calls `gpui_tokio::init(cx)` before constructing the transport.

## Behavior requirements
- **Cancellation:** dropping the task aborts (`gpui_tokio::Tokio::spawn` aborts its tokio task when the returned `Task` is dropped). UI holds `Option<Task<…>>`.
- **Timing:** record DNS, connect, TLS, time-to-first-byte, download. If the transport can't expose phases, show total only — don't fake them.
- **Redirects:** setting-driven (follow, max N); record the chain and show it in the Timeline tab.
- **TLS:** verify by default; per-request "disable verification" is visibly flagged in the UI. Client certs and custom CA per host.
- **Proxy:** system / manual / none; respect `NO_PROXY`.
- **Cookies:** per-workspace jar, viewable/editable; cookies are secrets for logging purposes.
- **Bodies:** stream request files (binary/form-data) from disk; stream responses to a temp file with an in-memory prefix
  (see `rust-gpui-ui-ux/references/performance.md`). Decompress gzip/br/deflate; keep raw bytes for Hex view.
- **Errors:** a typed `TransportError` (Dns, Connect, Tls, Timeout, TooManyRedirects, InvalidUrl, Io, Cancelled, Internal) with
  user-facing messages that suggest a fix. HTTP 4xx/5xx are **responses**, not errors.
- **Auth helpers:** Basic, Bearer, API key (header/query), OAuth 2.0 (auth code + PKCE via system browser, client credentials),
  AWS SigV4, Digest — each a pure function `apply(auth, &mut ResolvedRequest)` with unit tests against known vectors.
- **Scripts (pre-request/tests):** out of scope until a sandbox decision is made; imported scripts are stored but never executed silently.

## Security
- Never log full URLs with query secrets, headers like `Authorization`/`Cookie`/`Set-Cookie`/`X-Api-Key`, or bodies.
  Provide a single `redact()` used by console, history and error reports.
- Secret variable values are fetched from the keychain at resolve time and never written into `ResolvedRequest` debug output.
- Imported collections are untrusted: validate sizes, reject absurd nesting, no script execution.

## Testing
- Pure crates (`api_model`, `variables`, auth helpers): plain `#[test]`, table-driven, plus import/export fixtures.
- Engine: `FakeTransport` scripted responses/errors/delays (GPUI timer); assert cancellation and error mapping.
- A small number of integration tests against a local test server (spawned in-process on 127.0.0.1) for redirects, gzip, TLS errors.
