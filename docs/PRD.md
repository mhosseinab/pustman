# Pustman — Product Requirements Document (v1)

| | |
| --- | --- |
| Status | Draft for review |
| Date | 2026-11-17 |
| Supersedes | Scope of `docs/POSTMAN_RECREATION_PRD.md` (kept as the reverse-engineering evidence log) |
| Architecture | `AGENTS.md` (GPUI, crate-per-feature, Zed patterns) |

## 0. How to read this document

Pustman is a native Rust/GPUI API client. Version 1 covers exactly three things:

1. **Compose and send any common API request type**: HTTP/REST, GraphQL, Server-Sent Events, WebSocket, Socket.IO, gRPC and MQTT.
2. **Share workspaces with teammates.**
3. **Debug requests and responses**: see exactly what was sent and received, when, and why it failed.

Everything else Postman ships is out of scope (§12).

Each requirement has an ID, a priority and a source tag:

- **P0** — v1 cannot ship without it. **P1** — v1 target, can slip to v1.x. **P2** — post-v1, listed so the design does not block it.
- **[O]** Observed in the Postman 12.37.1 artifact (see `POSTMAN_RECREATION_PRD.md`, requirement ID given).
- **[K]** Postman product behavior that users expect, from product knowledge rather than the artifact.
- **[D]** Pustman design decision; rationale given where it is not obvious.

Acceptance criteria (**AC**) are written to be turned directly into GPUI or unit tests (`gpui-testing` skill).

---

## 1. Goals and non-goals

### Goals
- **G1** A Postman user can open Pustman, import their collections and environments, and send every request they could send before, for the supported protocols.
- **G2** A team can share a workspace through an ordinary Git repository with reviewable diffs, without any Pustman account or server.
- **G3** When a request fails or behaves unexpectedly, the user can find out why without leaving the app: the resolved request as sent, the connection and TLS details, timings, redirects, protocol frames and errors.
- **G4** Native performance: instant startup, no UI freezes on large responses or long message streams.

### Non-goals (v1)
- Cloud accounts, sign-in, hosted sync, or any Pustman-operated server.
- Executing pre-request or test scripts (§12; the design keeps room for it, REQ-SCRIPT-01).
- Collection runner, performance or load testing, monitors, mock servers, AI or agent features (§12).

### Success metrics
- Import of a Postman v2.1 collection succeeds for ≥ 99% of requests in a corpus of real public collections. Anything unsupported is reported item by item.
- Time from launch to an interactive window: **< 1 s** on the reference machine (4-core laptop, SSD), cold cache **< 2 s**.
- A 100 MB response body never blocks the UI thread for more than 16 ms per frame.
- Zero secret values in workspace files, logs or history (verified by the redaction tests in §10).

---

## 2. Users and core jobs

| Persona | Core jobs |
| --- | --- |
| Backend developer | Hit local and staging endpoints, iterate on payloads, inspect raw responses, debug auth and TLS. |
| API consumer / integrator | Explore a third-party API from imported collections, swap environments, share working requests. |
| QA engineer | Reproduce bugs with exact requests, capture full exchanges, attach them to tickets. |
| Team lead | Keep a canonical, reviewed set of requests and environments in the team repo. |

---

## 3. Product structure (UI shell)

The layout follows the Postman mental model, built with the `workspace` crate's Workspace, Pane, Item, Dock and Panel primitives (see `rust-gpui-ui-ux` › `references/postman-screens.md`).

```
┌──────────────────────────────────────────────────────────────────────────┐
│ Title bar: workspace name ▾ · git status · environment picker ▾ · ⚙      │
├──────────┬───────────────────────────────────────────────────────────────┤
│ Left dock│ Tabs: [GET Users] [WS chat] [gRPC Greeter] [+]                │
│ ─────────│ ┌───────────────────────────────────────────────────────────┐ │
│ Collect. │ │ Request editor (protocol-specific)                        │ │
│ Environ. │ ├───────────────────────────────────────────────────────────┤ │
│ History  │ │ Response / message viewer (protocol-specific)             │ │
│          │ └───────────────────────────────────────────────────────────┘ │
├──────────┴───────────────────────────────────────────────────────────────┤
│ Bottom dock: Console (debug log)                 Status bar: cookies · ⚡  │
└──────────────────────────────────────────────────────────────────────────┘
```

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-UI-01 | Left dock with Collections, Environments and History panels. Each is resizable and collapsible; its size and visibility persist per workspace. | P0 | [K] |
| REQ-UI-02 | Center pane of tabs, one tab per open request. Tabs are reorderable and closable, show a dirty-state dot, can be pinned, and are restored on restart. Splitting panes is P2. | P0 | [K] |
| REQ-UI-03 | "New" menu and palette entry to create HTTP, GraphQL, WebSocket, Socket.IO, gRPC or MQTT requests. | P0 | [K] |
| REQ-UI-04 | Bottom dock **Console** (§8.3). | P0 | [K][O §18] |
| REQ-UI-05 | Command palette (`Ctrl/Cmd-Shift-P`) lists every action with its keybinding. Quick open (`Ctrl/Cmd-P`) fuzzy-finds requests across the workspace. | P0 | [D] |
| REQ-UI-06 | Light and dark themes are bundled, and the app follows the OS appearance by default. Themes are JSON in `assets/themes/`. Postman's ten named themes are not reproduced. | P0 | [O REQ-UI-03][D] |
| REQ-UI-07 | Every interactive element is keyboard-reachable and has an accessible label (`ux-checklist.md`). | P0 | [D] |
| REQ-UI-08 | Settings UI edits `settings.json`. The JSON file stays the source of truth (`pustman-settings-persistence`). | P1 | [D] |

**Default keybindings** [K] (Cmd on macOS, Ctrl on Linux and Windows, defined in `assets/keymaps/`):

| Action | Binding |
| --- | --- |
| Send / connect | `Ctrl-Enter` |
| Save | `Ctrl-S` |
| New tab | `Ctrl-T` |
| Close tab | `Ctrl-W` |
| Focus URL bar | `Ctrl-L` |
| Toggle console | `Ctrl-Alt-C` |
| Toggle sidebar | `Ctrl-\` |
| Command palette | `Ctrl-Shift-P` |
| Quick open | `Ctrl-P` |
| Next / previous tab | `Ctrl-Tab` / `Ctrl-Shift-Tab` |

---

## 4. Data model (`api_model` crate)

These are plain serde types with no GPUI and no IO. Every item has a stable `id` (ULID) so that renames and moves produce small diffs [D].

```
Workspace { id, name, format_version }
Collection { id, name, description, auth, variables, children: [Folder|Request], order }
Folder     { id, name, description, auth, children, order }
Request    = HttpRequest | GraphqlRequest | WebSocketRequest | SocketIoRequest | GrpcRequest | MqttRequest
Environment{ id, name, variables }
Variable   { key, value, kind: Default|Secret, enabled, description }
Auth       = Inherit | None | Basic | Bearer | ApiKey | Digest | OAuth2 | AwsSigV4 | Jwt | OAuth1 | Ntlm | Unsupported(raw)
```

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-MODEL-01 | Auth is inherited Collection → Folder → Request, and each level can override or set "No auth". The effective auth source is shown in the Auth tab. | P0 | [K] |
| REQ-MODEL-02 | Unknown or unsupported fields from imports (scripts, unsupported auth types such as Hawk, ASAP or EdgeGrid, protocol profiles) are **preserved verbatim** and round-trip on export. Pustman never silently drops them. | P0 | [D] |
| REQ-MODEL-03 | Every request type has a `settings` block: per-request overrides of global request settings (§5.1, REQ-HTTP-09). | P0 | [O REQ-EXEC-05] |
| REQ-MODEL-04 | Requests store saved examples (a request plus a named response) for documentation and sharing. Mock serving of examples is out of scope. | P1 | [K] |

---

## 5. Request types

All request types share the following: variables resolved at send time (§7), inherited auth where the protocol allows it, a Console entry per send or connection (§8.3), history entries (§8.5), save to collection, and duplicate, rename and move.

**Session model** [D]. HTTP, GraphQL and gRPC unary are *exchanges*: one request, one response. SSE, WebSocket, Socket.IO, gRPC streaming and MQTT are *sessions*: they connect, produce a stream of timestamped events and end with a disconnect reason. Both render in the shared **Message log** component (§8.4), so debugging looks the same across protocols.

### 5.1 HTTP / REST (`http_engine`)

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-HTTP-01 | Methods: GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS, TRACE and CONNECT, plus any custom token-valid method. | P0 | [O REQ-EXEC-03][K] |
| REQ-HTTP-02 | URL bar with `{{var}}` highlighting. Query params stay in sync both ways with a Params table (key, value, description, enabled). Path variables `:name` get their own table. | P0 | [K] |
| REQ-HTTP-03 | Headers table with enable toggles and autocomplete of common header names and values. Auto-generated headers (Host, Content-Length, Content-Type from body mode, User-Agent, Accept, Accept-Encoding, Connection) are shown read-only and can each be disabled. | P0 | [K] |
| REQ-HTTP-04 | Body modes: none, `form-data` (text and file fields, per-field content type), `x-www-form-urlencoded`, raw (Text, JSON, XML, HTML, JavaScript; syntax-highlighted; JSON validated with inline error), binary (a file from disk), GraphQL (§5.2). | P0 | [O REQ-EXEC-03] |
| REQ-HTTP-05 | File references in bodies are stored relative to the workspace root when inside it, otherwise as absolute paths. A missing file fails the send with "File not found: <path>". | P0 | [O REQ-EXEC-07][D] |
| REQ-HTTP-06 | Auth types for v1: No auth, Inherit, Basic, Bearer, API key (header or query), Digest, OAuth 2.0, AWS Signature v4, JWT Bearer. OAuth 1.0 and NTLM are P1. Each helper is a pure `apply(auth, &mut ResolvedRequest)` function. | P0 | [O REQ-EXEC-04][D] |
| REQ-HTTP-07 | OAuth 2.0 grants: authorization code (with PKCE), client credentials, password, implicit (legacy, flagged as such). The browser flow uses the **system browser** plus a loopback redirect `http://127.0.0.1:<port>/callback`; there is no embedded webview. Tokens are stored in the keychain and refreshed automatically when a refresh token exists. | P0 | [O REQ-AUTH-02/03][D] |
| REQ-HTTP-08 | Cookies: a per-workspace cookie jar, applied and captured automatically, with a Manage Cookies dialog to view, add, edit and delete them per domain. The jar is **local only** and never shared. | P0 | [O REQ-EXEC-05][K] |
| REQ-HTTP-09 | Request settings, each a global default plus a per-request override: timeout; follow redirects; max redirects; follow original method on 301/302; keep `Authorization` across redirects; verify TLS; HTTP version (Auto, 1.1, 2); disable cookie jar; encode URL automatically. | P0 | [O REQ-EXEC-05] |
| REQ-HTTP-10 | Certificates: client certificates per host (PEM, or PFX with passphrase in the keychain), custom CA bundle, and TLS verification on by default. A disabled check shows a persistent warning badge on the request and in the Console. | P0 | [O REQ-SEC-02][AGENTS.md] |
| REQ-HTTP-11 | Proxy: system, manual (HTTP/HTTPS/SOCKS5, with optional auth stored in the keychain) or none; `NO_PROXY` respected; per-request bypass. | P0 | [O REQ-PROXY-02][D] |
| REQ-HTTP-12 | Cancel an in-flight request (button or `Esc`). Cancelling drops the task, which aborts the connection, and the Console records "Cancelled". | P0 | [D] |
| REQ-HTTP-13 | Import a cURL command (pasted into the URL bar, or via File → Import) and copy any request as cURL. Other code snippet languages are P2. | P0 | [K] |

**AC (HTTP)**
- Sending `POST {{base}}/users` with JSON body `{"name":"{{name}}"}` sends the resolved URL and body with `Content-Type: application/json`. The Console shows the resolved request exactly as sent.
- With redirects enabled (max 5), a chain of 6 redirects fails with `TooManyRedirects` and the full chain is shown.
- A self-signed server fails with a TLS error whose message offers "Disable verification for this request". Once disabled, the request succeeds and the warning badge is visible.
- Cancelling within 50 ms of send leaves no pending task, and the request is not retried.

### 5.2 GraphQL (`http_engine` + `graphql` module)

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-GQL-01 | A dedicated GraphQL request type: URL, headers, auth, a query editor, a variables JSON editor and an operation-name picker (for documents with several operations). It is sent as `POST application/json {query, variables, operationName}`; GET is optional. | P0 | [O REQ-EXEC-03][K] |
| REQ-GQL-02 | Schema introspection on demand, cached per URL in the local store. It powers field autocomplete, hover docs and validation errors in the query editor. | P1 | [K] |
| REQ-GQL-03 | A schema explorer tree that inserts fields into the query. | P2 | [K] |
| REQ-GQL-04 | Subscriptions over WebSocket using the `graphql-transport-ws` protocol, rendered in the Message log. | P2 | [K] |
| REQ-GQL-05 | The response viewer highlights the `errors[]` array separately from `data`, even when the HTTP status is 200. | P0 | [D] |

### 5.3 Server-Sent Events

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-SSE-01 | An HTTP response with `Content-Type: text/event-stream` switches the response viewer into a live **event stream**: each event (`event`, `id`, `data`, `retry`) becomes a Message log row with a timestamp, until the server closes the stream or the user stops it. | P0 | [O REQ-EXEC-05 `isSSEStream`] |
| REQ-SSE-02 | Raw stream view: the exact bytes received. | P1 | [D] |
| REQ-SSE-03 | Automatic reconnect with `Last-Event-ID` is off by default, with an option to enable it. | P2 | [K] |

### 5.4 WebSocket (`websocket_client`)

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-WS-01 | Connect to `ws://` or `wss://` with custom headers, query params, subprotocols (`Sec-WebSocket-Protocol`), auth (Basic, Bearer or API key as headers or query), and the TLS, certificate and proxy settings of §5.1. | P0 | [O REQ-EXEC-10][K] |
| REQ-WS-02 | The handshake is shown in the Console: the request and the `101` response headers; a failed upgrade shows the HTTP status and body. | P0 | [D] |
| REQ-WS-03 | Compose and send Text, JSON (validated) or Binary messages (Base64, hex or a file). Messages can be saved by name within the request and resent with one click. | P0 | [K] |
| REQ-WS-04 | The Message log shows direction (↑ sent, ↓ received), timestamp, size, type and a payload preview; expanding a row pretty-prints JSON. It supports search and filtering by direction or text, and pausing the auto-scroll. | P0 | [K] |
| REQ-WS-05 | Disconnect with a chosen close code and reason. A server close shows its code and reason. Ping and pong frames are visible behind a toggle. | P0 | [K] |
| REQ-WS-06 | Connection settings: handshake timeout, reconnect attempts (default 0), max message size. | P1 | [K] |

### 5.5 Socket.IO (`websocket_client`, `socketio` module)

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-SIO-01 | Connect with a client version (v4 required, v3 if the library supports it, v2 P2), path (default `/socket.io`), namespace, headers, auth payload and handshake timeout. | P0 | [O REQ-EXEC-10][K] |
| REQ-SIO-02 | Emit events: event name plus one or more arguments (Text, JSON or Binary), with an optional **acknowledgement**; the ack reply is logged and linked to the emit. | P0 | [K] |
| REQ-SIO-03 | Listen to named events: a list of event names to subscribe to, with all-events mode on by default. Received events go to the Message log with their event name. | P0 | [K] |
| REQ-SIO-04 | Connection lifecycle events (`connect`, `connect_error`, `disconnect` with reason) are logged. | P0 | [K] |

### 5.6 gRPC (`grpc_client`)

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-GRPC-01 | Service definition comes from one of: (a) **server reflection** (`grpc.reflection.v1`, falling back to `v1alpha`); (b) **`.proto` files** imported into the workspace's `protos/` folder with import paths; (c) a FileDescriptorSet file. Protos are compiled at runtime, with no `protoc` dependency [D]. | P0 | [O REQ-EXEC-10][K] |
| REQ-GRPC-02 | Method picker lists services and methods with their streaming kind. All four kinds are supported: unary, server streaming, client streaming, bidirectional. | P0 | [K] |
| REQ-GRPC-03 | Messages are edited as JSON (proto3 JSON mapping), with "Generate example message" from the descriptor and inline validation against the message type. | P0 | [K] |
| REQ-GRPC-04 | Metadata table (ASCII and `-bin` keys), auth (Bearer, Basic or API key as metadata), deadline and timeout, TLS on or off, client certificates and CA (§5.1). Plaintext HTTP/2 (h2c) for local servers. | P0 | [K] |
| REQ-GRPC-05 | The response shows status code (name and number), status message, response headers metadata, trailers, and decoded `google.rpc.Status` details when present. | P0 | [K] |
| REQ-GRPC-06 | Streaming calls use the Message log: send messages on client and bidirectional streams, end the stream (half-close), cancel. | P0 | [K] |
| REQ-GRPC-07 | gRPC-Web and gRPC over HTTP/1.1 proxies. | P2 | [D] |

**AC (gRPC):** against a reflection-enabled test server, the user can pick `helloworld.Greeter/SayHello`, generate an example message, send it, and see `OK (0)` with headers and trailers. Removing the server's reflection gives a clear "Server reflection not available — import .proto files" message.

### 5.7 MQTT (`mqtt_client`)

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-MQTT-01 | Connect to `mqtt://`, `mqtts://`, `ws://` or `wss://` brokers, MQTT **3.1.1 and 5.0**. Connection options: client ID (generated by default), username and password (password in the keychain), clean session or clean start, keep-alive, last will (topic, payload, QoS, retain), and TLS and certificates (§5.1). | P0 | [O REQ-EXEC-10][K] |
| REQ-MQTT-02 | Subscribe to topics with wildcards (`+`, `#`) and QoS 0, 1 or 2. Subscriptions are saved with the request and can be toggled live. | P0 | [K] |
| REQ-MQTT-03 | Publish to a topic with payload (Text, JSON or Binary), QoS and retain; MQTT 5 user properties, content type and message expiry. | P0 | [K] |
| REQ-MQTT-04 | The Message log shows topic, QoS, retain flag, properties and payload; filtering by topic is supported. | P0 | [K] |

---

## 6. Workspaces and team sharing

**Decision [D]:** a workspace is a **folder of plain files**, and teams share it by putting that folder in a **Git repository**. Postman itself is moving the same way (native Git, file-system workspaces: [O REQ-FS-01..03, REQ-GIT-01..03]). This gives sharing, review, history and permissions through tools teams already have, with no server to build or operate. Real-time cloud sync is P2 (§6.5).

> This replaces the earlier note in the `pustman-settings-persistence` skill ("Postman v2.1-compatible JSON" as the on-disk format). v2.1 is one file per collection, which conflicts on every concurrent edit, and it cannot represent gRPC, WebSocket or MQTT requests. v2.1 stays as the **import/export** format (§6.4). Update that skill when this PRD is accepted.

### 6.1 On-disk format

```
my-api-workspace/
  pustman.json                          # { id, name, format_version }
  .pustman/settings.json                # shared workspace settings (layered over user settings)
  collections/
    users-api/
      collection.json                   # metadata, auth, variables, child order
      auth/
        folder.json
        login.request.json
      get-user.request.json
      chat.websocket.json
      greeter.grpc.json
  environments/
    staging.environment.json
    production.environment.json
  protos/                               # .proto files referenced by gRPC requests
```

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-WSP-01 | One file per request, folder, collection and environment. The file extension encodes the item type. Child order is stored in the parent's `order` array (by ID), never inferred from directory listing order. | P0 | [D] |
| REQ-WSP-02 | Files are deterministic: pretty-printed JSON, two-space indent, fixed key order, trailing newline, maps sorted. Saving an unchanged item produces a zero-byte diff. Writes are atomic (temp file plus rename). | P0 | [D] |
| REQ-WSP-03 | File names are slugs of item names, with a `-2`, `-3` suffix on collision. Identity is the `id` inside the file, so renaming an item renames its file and Git shows a rename. | P0 | [D] |
| REQ-WSP-04 | `format_version` is checked on open. A newer major version opens read-only with a banner; older versions are migrated in memory and written back only on save. | P0 | [D] |
| REQ-WSP-05 | Open, create and recently-opened workspaces. Several workspaces can be open, one per window. | P0 | [K][O REQ-FS-06] |
| REQ-WSP-06 | Workspace files contain **no secrets and no local state**. History, open tabs, layout, cookies, OAuth tokens, response cache and *local variable values* live in the per-user local store (SQLite plus keychain), keyed by workspace `id`. Pustman never writes into the repo except on an explicit save. | P0 | [D][AGENTS.md Security] |

### 6.2 Shared values vs local values

This mirrors Postman's initial/current split [K], made explicit for Git.

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-VAR-SHARE-01 | Each variable has a **shared value** (written to the file, visible to the team) and an optional **local value** (in the local store, never committed). Resolution uses the local value when set. The variables table shows both columns, with "Reset to shared" and "Promote local to shared" actions. | P0 | [K][D] |
| REQ-VAR-SHARE-02 | **Secret** variables never have a shared value. The file records only `{key, kind: "secret", description}`; each teammate enters their own value, which is stored in the OS keychain. A teammate who has not set a secret gets an "unresolved secret" marker before send, never an empty string sent silently. | P0 | [O REQ-AUTH-04][D] |
| REQ-VAR-SHARE-03 | Editing a request in a way that would write a literal token into a shared field (Authorization header, bearer token, password, API key) shows a one-time nudge to use a secret variable instead. It never blocks the edit. | P1 | [D] |

### 6.3 Collaboration through Git

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-COLLAB-01 | **External changes** (`git pull`, branch switch, a teammate's edit on a shared drive) are picked up by a file watcher and reflected in the tree and open tabs within 1 s. Unsaved edits in an open tab are kept, and a "Changed on disk — Reload / Keep mine / Compare" bar appears. | P0 | [O REQ-FS-03][D] |
| REQ-COLLAB-02 | Files with merge-conflict markers (`^(<{7}\|>{7})( \|$)`) or invalid JSON are shown with an error badge and a message naming the file and line. They are never silently skipped or overwritten. | P0 | [O REQ-GIT-03] |
| REQ-COLLAB-03 | Status bar shows the Git branch and dirty count when the workspace is inside a repository. Git is invoked through the `git` CLI (`smol::process`); there is no embedded Git library [D]. | P0 | [O REQ-GIT-02] |
| REQ-COLLAB-04 | **Git panel:** changed items listed by request name (not file path), item-level diff (old vs new request fields), stage and commit selected items, pull, push, fetch, switch and create branch. Auth uses the user's existing Git credential setup. | P1 | [O REQ-GIT-01][D] |
| REQ-COLLAB-05 | "Share request" copies a single request as a Pustman JSON snippet or a cURL command to the clipboard; pasting either into a collection imports it. | P1 | [K] |
| REQ-COLLAB-06 | `.gitignore` guidance: creating a workspace inside a repository offers to add nothing (there are no local files in the workspace) and confirms that secrets are not stored there. | P1 | [D] |

**AC (sharing):** Teammate A adds a request and a secret variable `api_key`, commits and pushes. Teammate B pulls while Pustman is open. Within 1 s the request appears in B's tree, `api_key` shows as unset, and sending prompts B to set it. `git diff` of A's commit contains no secret value.

### 6.4 Import and export

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-IO-01 | Import a Postman Collection **v2.1** (and v2.0) JSON file into a workspace. The report lists imported items, items with preserved-but-inactive scripts, and unsupported auth types or settings. | P0 | [O REQ-MIG-02][K] |
| REQ-IO-02 | Import Postman environment and globals JSON. Postman `secret`-typed values are imported into the keychain, not into files. | P0 | [K] |
| REQ-IO-03 | Export a collection as Postman v2.1. Request types v2.1 cannot represent (gRPC, WebSocket, Socket.IO, MQTT) are skipped with a warning listing them. | P0 | [D] |
| REQ-IO-04 | Import OpenAPI 3.x and Swagger 2.0 as a collection (one request per operation, examples as bodies). | P1 | [K] |
| REQ-IO-05 | Import a whole Postman data dump (several collections and environments) in one step. | P2 | [K] |
| REQ-IO-06 | Imported data is untrusted: size limits (50 MB per file by default), nesting depth limit, and **no script execution**. | P0 | [AGENTS.md Security] |

### 6.5 Future: real-time team sync (P2, decision deferred)

A self-hostable sync service (CRDT- or operation-based, with per-workspace access control) can sit beside the file format; the stable IDs in REQ-WSP-01 are what make that possible. It is out of v1 because it requires accounts, a server, conflict semantics and operations that the Git model avoids. Revisit when a team reports Git as a blocker.

---

## 7. Variables (`variables` crate)

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-VAR-01 | `{{name}}` syntax works in the URL, params, headers, body (all modes), auth fields, GraphQL variables, gRPC metadata and messages, WebSocket and Socket.IO messages, and MQTT topics and payloads. | P0 | [K] |
| REQ-VAR-02 | Scope precedence, narrowest wins: **request-local → environment → collection → global**. A data scope exists in the resolver but is unused in v1 (there is no runner). | P0 | [K][skill: pustman-http-engine] |
| REQ-VAR-03 | Highlighting in every editor: resolved (green), unresolved (red), secret (masked). Hover shows the value (masked for secrets, with a reveal toggle), its scope and its source file. | P0 | [K] |
| REQ-VAR-04 | Autocomplete of variable names after `{{`, across all scopes in effect for the request. | P0 | [K] |
| REQ-VAR-05 | Dynamic variables `{{$guid}}`, `{{$timestamp}}`, `{{$isoTimestamp}}`, `{{$randomInt}}`, `{{$randomUUID}}`, plus a documented small set of `$random*` generators. They are resolved once per send, so the same name gets the same value throughout one request. | P0 | [K] |
| REQ-VAR-06 | Cycle detection (`a → b → a`) produces an error naming the cycle. Nested references are resolved to a depth of 10. | P0 | [D] |
| REQ-VAR-07 | Pre-send check: if a send would use unresolved variables, a non-blocking warning lists them. Sending anyway is allowed and is logged in the Console. | P0 | [D] |
| REQ-VAR-08 | Environment picker in the title bar, plus a quick-look popover listing the active environment's and globals' values. The active environment is stored per workspace locally. | P0 | [K] |

---

## 8. Debugging requests and responses

This is the core differentiator. Every protocol produces the same three artifacts: the **resolved request** (what was actually sent), the **result** (response or event stream), and a **Console entry** with timing, network and error detail.

### 8.1 HTTP response viewer (`response_viewer`)

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-RESP-01 | Status line: code, reason phrase (color by class), total time, size (headers and body, compressed and decompressed), HTTP version. Each item opens a breakdown popover on hover or focus. | P0 | [K] |
| REQ-RESP-02 | Body views: **Pretty** (JSON, XML, HTML, JavaScript, CSS, auto-detected from content type with a manual override; folding; line numbers), **Raw** (exact decoded text), **Hex** (raw bytes, before decompression available), **Preview** (images; HTML rendered as source text with a note, since a native app has no web engine; PDF opens externally). | P0 | [K][D] |
| REQ-RESP-03 | Find in body (`Ctrl-F`), with regex and case-sensitivity options. | P0 | [K] |
| REQ-RESP-04 | JSON filter: a JSONPath expression narrows the Pretty view. | P1 | [K] |
| REQ-RESP-05 | Headers table (with duplicates kept in order), Cookies table (set by this response, with all attributes), **Timeline** tab (§8.2). | P0 | [K] |
| REQ-RESP-06 | Large responses: the body streams to a temp file with an in-memory prefix. Pretty mode is limited to 10 MB (configurable), above which Raw opens with a notice. Views are virtualized; nothing blocks the UI thread. | P0 | [D][perf skill] |
| REQ-RESP-07 | Save the response body to a file, copy the body, copy one header, and save the request plus response as an example. | P0 | [K] |
| REQ-RESP-08 | Errors are not empty responses: transport failures render an error panel with type, message, suggested fix and a link to the Console entry (§8.6). | P0 | [D] |
| REQ-RESP-09 | Compare a response with a previous one from History or a saved example (side-by-side diff). | P2 | [D] |

### 8.2 Timeline and network details

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-NET-01 | Timing phases where the transport exposes them: socket wait / prepare, DNS lookup, TCP connect, TLS handshake, time to first byte, download, total. Phases that cannot be measured are **omitted, never estimated**. Connection reuse is shown ("reused connection, 0 ms connect"). | P0 | [skill: pustman-http-engine] |
| REQ-NET-02 | Network info: remote IP and port, local address, HTTP version negotiated (ALPN), TLS protocol and cipher, server certificate chain (subject, issuer, validity, SANs, SHA-256 fingerprint), whether TLS verification was on. | P0 | [K] |
| REQ-NET-03 | Redirect chain: each hop's method, URL, status, response headers and timing, plus the `Location` that was followed. | P0 | [O REQ-EXEC-05] |
| REQ-NET-04 | **Resolved request as sent**: final URL, method, every header including auto-generated, auth-applied and cookie headers (secrets masked by default, reveal per value), and the body as sent (multipart boundaries visible). | P0 | [K] |

**Spike required** [D]: confirm which phases `reqwest` exposes on our pinned version (custom DNS resolver timing, connector layer for connect and TLS). If phase timing needs a hyper-level connector, do that inside `ReqwestTransport` only (`pustman-http-engine` › Client abstraction).

### 8.3 Console (`console` crate, bottom dock)

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-CON-01 | One entry per HTTP, GraphQL or gRPC-unary send and per session (WS, Socket.IO, gRPC stream, MQTT, SSE). An entry is a summary line (method/protocol, URL, status, time, size) that expands to the resolved request, the response or events, the network details and the errors. | P0 | [K][O §18] |
| REQ-CON-02 | Filter by level (info, warning, error), protocol and text; clear; pause. The buffer is capped (default 5,000 entries, configurable), with the oldest dropped first. | P0 | [K] |
| REQ-CON-03 | Show or hide raw view per entry: the exact HTTP/1.1 request and response text as on the wire (HTTP/2 rendered as pseudo-headers plus headers). | P0 | [K] |
| REQ-CON-04 | Warnings land in the Console: unresolved variables, TLS verification disabled, request body file missing, imported script not executed, and response decompression failure. | P0 | [D] |
| REQ-CON-05 | Export selected entries as a **HAR 1.2** file, with secrets redacted unless the user explicitly includes them through a confirmation. | P1 | [D] |
| REQ-CON-06 | Console content is in memory only. It is never written to disk unless exported. | P0 | [AGENTS.md Security] |

### 8.4 Message log (shared by all session protocols)

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-LOG-01 | A virtualized list of events: direction (sent, received or system), timestamp (ms, absolute or relative to connect), type, size and preview. Lifecycle events (connecting, connected, handshake details, error, disconnected with reason) are system rows. | P0 | [D] |
| REQ-LOG-02 | Expand a row for full payload: JSON pretty, text, hex for binary, and protocol-specific fields (MQTT topic, QoS, properties; Socket.IO event name and ack ID; gRPC message index). | P0 | [D] |
| REQ-LOG-03 | Search, filter (direction, type, text), pause auto-scroll, clear, copy payload, and export the session as JSON lines. | P0 | [K] |
| REQ-LOG-04 | Remains smooth with 100,000 events: the list is virtualized and payloads over 1 MB are stored off-heap and previewed. | P0 | [D] |

### 8.5 History (`history_panel`)

| ID | Requirement | Pri | Src |
| --- | --- | --- | --- |
| REQ-HIST-01 | Every send is recorded in the local store with its timestamp, protocol, method/target, status and duration, grouped by day. History is per workspace. | P0 | [K][O REQ-MIG-02] |
| REQ-HIST-02 | Opening a history entry restores the request as it was **before variable resolution** and shows the stored response summary. Bodies are stored up to 1 MB by default (configurable; 0 disables). | P0 | [D] |
| REQ-HIST-03 | Secrets are redacted before storage (Authorization, cookies, secret variable values, OAuth tokens). | P0 | [AGENTS.md Security] |
| REQ-HIST-04 | Search history, save an entry to a collection, delete entries, clear all, and a retention setting (default 30 days). | P0 | [K] |

### 8.6 Error taxonomy and messages

Typed errors map to user-facing messages with a suggested fix. These are shared by all protocols where applicable. [O §6.1][skill]

| Error | Example message and suggestion |
| --- | --- |
| `InvalidUrl` | "Invalid URL after variable resolution: `{{base}}/x` → `/x`. Is `base` set in the active environment?" |
| `Dns` | "Could not resolve `api.staging.local`. Check the host name, VPN, or proxy settings." |
| `Connect` | "Connection refused at 127.0.0.1:8080. Is the server running?" |
| `Tls` | "Certificate not trusted (self-signed). Add a CA certificate or disable verification for this request." |
| `Timeout` | "No response within 30 s (request timeout). Change it in request settings." |
| `TooManyRedirects` | "Stopped after 10 redirects." Shows the chain. |
| `Proxy` | "Proxy authentication required (407)." |
| `Protocol` | "Server does not support HTTP/2" / "WebSocket upgrade rejected (403)" / "MQTT CONNACK: not authorized (5)" / "gRPC UNAVAILABLE". |
| `Io` / `FileNotFound` | "Body file not found: ./fixtures/avatar.png" |
| `UnresolvedSecret` | "Secret `api_key` has no local value. Set it in Environments → staging." |
| `Cancelled` | (info level) "Cancelled by user." |

HTTP 4xx and 5xx responses and non-OK gRPC statuses are **results, not errors**: they render normally, with status coloring.

---

## 9. Architecture mapping (adapts `AGENTS.md` › Proposed architecture)

New crates relative to `AGENTS.md` are marked ★. Each protocol crate is a domain crate with no GPUI views, tested against fakes or local in-process servers.

| Crate | Responsibility in this PRD | Key reqs |
| --- | --- | --- |
| `api_model` | Types for every request kind, collections, environments, auth; Postman v2.1, OpenAPI and cURL import/export | §4, §6.4 |
| `variables` | Resolution, scopes, dynamic vars, cycle detection, spans for highlighting | §7 |
| `http_engine` | HTTP, GraphQL and SSE transport (reqwest on `gpui_tokio`), auth helpers, cookies, TLS, proxy, redirects, timing | §5.1–5.3, §8.2 |
| `websocket_client` ★ | Raw WebSocket plus Socket.IO sessions emitting `SessionEvent`s | §5.4–5.5 |
| `grpc_client` ★ | Reflection client, runtime proto compilation, dynamic messages, the four call kinds | §5.6 |
| `mqtt_client` ★ | MQTT 3.1.1 and 5 sessions | §5.7 |
| `workspace_store` ★ | On-disk workspace format, atomic writes, file watcher, conflict detection, item ↔ file mapping | §6.1–6.3 |
| `git` ★ (P1) | Thin wrapper over the `git` CLI: status, diff, commit, pull, push, branch | REQ-COLLAB-03/04 |
| `storage` | SQLite local store: history, tabs, layout, local variable values, cookie jar, schema caches | §6.1 REQ-WSP-06, §8.5 |
| `secrets` ★ (or a module of `storage`) | OS keychain access for secret variables, OAuth tokens, certificate passphrases, proxy credentials | REQ-VAR-SHARE-02 |
| `console` ★ | Console entries, ring buffer, HAR export, redaction | §8.3 |
| `request_editor` | Protocol-specific editors (one Item type per protocol, sharing the URL bar, tables and auth components) | §5 |
| `response_viewer` | HTTP response views, timeline, shared **Message log** component | §8.1, §8.4 |
| `collections_panel`, `environment_picker`, `history_panel`, `command_palette` | As in `AGENTS.md` | §3, §7, §8.5 |

**Common session interface** [D]. Every streaming protocol crate exposes the same shape, so `response_viewer`'s Message log and `console` stay protocol-agnostic:

```rust
pub enum SessionEvent {
    Connecting { target: String },
    Connected { details: ConnectionDetails },   // handshake headers, negotiated protocol, TLS info
    Sent(Message),
    Received(Message),
    Error(SessionError),
    Closed { reason: CloseReason },
}
// connect() returns a handle (send / close) plus a stream of SessionEvent.
// Dropping the GPUI Task that drives the stream closes the connection.
```

**Candidate libraries.** Verify each with Context7 and against our toolchain before adoption, in the first commit that adds the crate:

| Need | Candidate |
| --- | --- |
| HTTP | `reqwest` (already decided) |
| WebSocket | `tokio-tungstenite` |
| Socket.IO | `rust_socketio` (check which protocol versions it speaks; REQ-SIO-01 v3 and v2 depend on it) |
| gRPC | `tonic` with `prost-reflect` (dynamic messages) and `protox` (pure-Rust `.proto` compilation); reflection protos via `tonic-reflection` |
| MQTT | `rumqttc` (v4 and v5 clients) |
| File watching | `notify` |
| Keychain | `keyring` |
| IDs | `ulid` |

---

## 10. Non-functional requirements

| ID | Requirement | Pri |
| --- | --- | --- |
| REQ-NFR-01 | Platforms: Linux (X11 and Wayland), macOS, Windows. Paths, keychains and keymaps are per platform. | P0 |
| REQ-NFR-02 | Performance: launch to interactive < 1 s (§1). Tree of 10,000 requests scrolls at 60 fps. Typing in editors has < 16 ms latency. No blocking IO on the foreground thread (`AGENTS.md`). | P0 |
| REQ-NFR-03 | Resource limits: concurrent sessions are limited only by the OS, with each idle session using < 1 MB. Response body memory is bounded by the in-memory prefix (default 10 MB) and the rest goes to temp files, which are deleted on tab close and on startup. | P0 |
| REQ-NFR-04 | Security: no logging of bodies, auth headers, cookies or secret values. One shared `redact()` for Console export, history and error reports. TLS verification on by default. Imported scripts are never executed. No telemetry in v1, and no network calls other than user-initiated requests and the user's own `git`. | P0 |
| REQ-NFR-05 | Reliability: autosave of unsaved tab drafts to the local store every 2 s (restored after a crash). Workspace files are written only on explicit save. | P0 |
| REQ-NFR-06 | Accessibility: full keyboard operation, visible focus, 4.5:1 text contrast in bundled themes, screen-reader labels (as far as GPUI supports them; gaps documented). | P0 |
| REQ-NFR-07 | Offline-first: every feature works without internet access except sending to remote hosts and Git remote operations. | P0 |

**Redaction test (required):** a fixture workspace with secrets in every location (headers, auth, variables, cookies, OAuth tokens). Send, save, record history and export HAR, then assert that the secret values appear in no file under the workspace, the SQLite database or the export.

---

## 11. Milestones

Each milestone ends with its acceptance criteria passing in CI (`cargo test --workspace`, clippy, fmt) and a manual run-through on Linux and macOS.

| # | Milestone | Contents | Exit criteria |
| --- | --- | --- | --- |
| M0 | Shell | Workspace window, docks, tabs, theme, settings, keymaps, command palette, local store | App launches < 1 s; tabs and layout restore after restart |
| M1 | HTTP core + debugging | §5.1, §7, §8.1–8.3, §8.5, §8.6; Basic, Bearer and API-key auth; cookies; cURL | HTTP ACs pass; redaction test passes |
| M2 | Workspaces + sharing | §6.1–6.4 (without the P1 Git panel), v2.1 import and export, secrets in keychain | Sharing AC passes with two clones of one repo |
| M3 | GraphQL + SSE + full auth | §5.2 (P0), §5.3, OAuth 2.0, Digest, AWS SigV4, JWT, client certs, proxy | OAuth 2.0 PKCE against a local test IdP; SSE stream of 10k events smooth |
| M4 | WebSocket + Socket.IO | §5.4, §5.5, Message log §8.4 | Echo server and Socket.IO test server ACs; 100k-event log smooth |
| M5 | gRPC | §5.6 | gRPC AC passes for reflection and proto import, all four call kinds |
| M6 | MQTT | §5.7 | Pub/sub round trip against a local broker for v3.1.1 and v5 |
| M7 | v1 hardening | P1 items: Git panel, GraphQL introspection, OpenAPI import, HAR export, OAuth 1.0, NTLM | Postman v2.1 import corpus ≥ 99%; accessibility checklist pass |

M4–M6 are independent once M1 and M2 land and can run in parallel.

---

## 12. Explicitly out of scope

The following Postman features, many of them visible in the artifact, are removed from Pustman's scope. Do not build hooks for them unless a requirement above names one.

| Removed | Artifact evidence | Reason |
| --- | --- | --- |
| Pre-request and test scripts (`pm.*` sandbox) | REQ-EXEC-02/06, REQ-UI-02 | Needs a sandboxed JS engine and a security decision. **REQ-SCRIPT-01 (P2):** scripts are preserved on import and shown read-only with "not executed" so a later engine can run them. |
| Collection runner, performance and load testing, DuckDB analytics | §7 | Not requested |
| Mock servers and simulations | §10 | Not requested |
| MCP servers, AI agent, Desktop and Cloud Agent, browser recorder | §8, §12 | Not requested |
| Datasets engine, JDBC | §13 | Not requested |
| Interceptor, capture proxy, webhook tunnels, captive-portal web proxy | §9, §11 | Not requested; the *outgoing* proxy (REQ-HTTP-11) is kept |
| Postman accounts, cloud workspaces, sign-in, payments | REQ-AUTH-01, §3 | Sharing is Git-based (§6) |
| External vaults (Azure Key Vault, AWS Secrets Manager, HashiCorp, 1Password) | REQ-AUTH-04 | OS keychain only in v1; P2 candidate |
| Visualizer, API design and specs, documentation publishing, monitors, flows | various | Not requested |
| Telemetry, analytics, crash upload | §18 | Privacy; local logs only |
| Legacy migrations (file-protocol, shell, scratchpad) | §17 | Postman-internal history; import (§6.4) covers user data |
| Restricted-address blocking | REQ-SEC-01 | Cloud-runner policy; a desktop client must reach localhost and private networks |

---

## 13. Open decisions

| # | Decision | Recommendation | Owner and deadline |
| --- | --- | --- | --- |
| D1 | On-disk item format: JSON vs YAML (raw bodies diff poorly in JSON strings) | JSON for v1 (no extra parser; serde_json already used). Revisit with real diffs after M2. | Before M2 |
| D2 | Timing phases from reqwest vs a custom hyper connector | Spike in M1 (§8.2) | M1 week 1 |
| D3 | Socket.IO v2 and v3 support depends on the client library | Ship v4; add the others only if the library supports them | M4 |
| D4 | Scripting engine (boa / rquickjs / none) and sandbox model | Out of v1; decide after v1 feedback | Post-v1 |
| D5 | Real-time sync service | Defer (§6.5) | Post-v1 |
| D6 | SQLite binding (`rusqlite` vs `sqlx`) | Already open in `pustman-settings-persistence`; decide in M0 | M0 |
