# Postman behaviour evidence log

**Subject:** the Postman 12.37.1 desktop app for Linux x64, as shipped.
**Purpose:** a record of the *user-visible behaviour* observed in that app. `docs/PRD.md` cites these
requirement IDs as its **[O]** evidence. The scope of this file is now covered by `docs/PRD.md`, which
replaces it. This file is kept only so that its IDs resolve.

This log records behaviour and capabilities only. It deliberately leaves out Postman's internal module
names, IPC channels, configuration keys, endpoints, protocol details and identifiers. Pustman is an
independent implementation and needs none of them.

## Evidence labels
- **[O]** Observed: read statically in the shipped app. No code was run.
- **[I:high|med|low]** Inferred from observed facts, with a stated confidence.
- **[U]** Unknown: not established. It must be discovered before it becomes a hard requirement.

Absence of evidence is not treated as absence of behaviour.

---

## 1. Product overview
A cross-platform desktop API client with several processes: one coordinating process, a set of UI
windows, and background workers. The workers handle request execution, proxying, mock servers,
MCP servers and datasets.

## 2. Architecture
- **REQ-ARCH-01 [O]** Separate processes for the UI, request execution and long-running services.
- **REQ-ARCH-02 [O]** Processes talk to each other only by message passing.
- **REQ-ARCH-03 [O]** All windows share common services: logging, an event bus, token refresh,
  migration, toasts and execution.
- **REQ-ARCH-04 [O]** Behaviour is configured at build time through a typed configuration surface.

## 3. Configuration
- **REQ-CFG-01 [O]** Network endpoints come from configuration and are not hard-coded. Build-time
  flags also control telemetry, crash reporting, release channel and feature gates.
- **REQ-CFG-02 [O]** An installation profile records the user's name, company and install options.

## 4. Bootstrap and window lifecycle
- **REQ-BOOT-01 [O]** Logs go to the console and to rotating files, and optionally to remote sinks.
- **REQ-BOOT-02 [O]** An app-booted signal is raised once the main workspace is ready.
- **REQ-BOOT-03 [O]** When several windows are open, one is elected leader to coordinate shared work,
  and leadership is handed over when that window closes.
- **REQ-BOOT-04 [O]** Distinct window kinds: main workspace, offline-only workspace, console, offline
  notice, and the auth and proxy-auth dialogs.
- **REQ-BOOT-05 [O]** A default working directory under the user's home folder.
- **REQ-BOOT-06 [O]** Developer tooling is installed into the app. **[U]** whether this happens in
  production builds.

## 5. UI shell
- **REQ-UI-01 [O]** Separate surfaces: requester, collection runner, offline workspace, console, script
  sandbox, proxy sign-in, auth, loading screen, migration screens and offline notice.
- **REQ-UI-02 [O]** Test scripts run in an isolated sandbox. They can see the request, response,
  iteration data, environments and globals, and they cannot open windows.
- **REQ-UI-03 [O]** Ten named colour themes in light, dark and high-contrast variants, built on semantic
  colour tokens such as brand and primary content.
- **REQ-UI-04 [O]** Narrow, per-window bridges for payments, TLS checks and the response visualizer.
  Dragging files onto the window is blocked.
- **REQ-UI-05 [O]** The app can switch to a remotely delivered UI version.
- **REQ-UI-06 [I:med]** It is unclear where some surfaces are hosted, such as the runner, the loading
  screen and the script sandbox.

## 6. Request execution
- **REQ-EXEC-01 [O]** A layered engine: collection model, HTTP client, run orchestration and script
  sandbox.
- **REQ-EXEC-02 [O]** Each request runs its pre-request scripts, then sends, then runs its test scripts.
  Collection runs can stop on a script error or on a failed test. Scripts can choose the next request
  to run, or run another request, with a limit on how deep that nesting can go.
- **REQ-EXEC-03 [O]** Collection runs use HTTP(S). Body modes: raw, URL-encoded, form-data, file and
  GraphQL. Methods: GET, HEAD, POST, PUT, PATCH, DELETE, OPTIONS, CONNECT and TRACE.
- **REQ-EXEC-04 [O]** Auth types: none, Basic, Bearer, Digest, Hawk, OAuth 1.0, OAuth 2.0, ASAP,
  NTLM, API key, AWS Signature v4, EdgeGrid and JWT.
- **REQ-EXEC-05 [O]** Per-request settings: request timeout and overall timeout, strict TLS, follow
  redirects, maximum redirects, follow the original method, keep `Authorization` across redirects,
  disable the cookie jar, and a client certificate. A cookie jar is applied automatically.
  Server-sent event streams are detected by content type.
- **REQ-EXEC-06 [O]** Scripts run in a sandbox with no access to module loading or `eval`, and with
  timeouts for both synchronous and asynchronous execution.
- **REQ-EXEC-07 [O]** File bodies are read from disk or from cloud storage. Local file access can be
  turned off. A missing or unreadable file fails the request with an error naming the file.
- **REQ-EXEC-08 [O]** A run that references a missing or deleted request fails with a clear error.
- **REQ-EXEC-09 [O]** Scripts can read files only inside the working directory.
- **REQ-EXEC-10 [I:high]** WebSocket, gRPC, MQTT and Socket.IO are available when sending
  interactively but are not part of collection runs. **[U]** whether a collection run can include
  requests that are not HTTP.
- **§6.1 Errors [O]** Errors are reported as distinct kinds: transport, HTTP, script, file load,
  assertion, secret resolution, secret configuration and worker. Timeouts are reported as such.

## 7. Collection runner and performance testing
- **REQ-PERF-01 [O]** Load profiles: fixed, ramp-up, spike and peak, each built from control points,
  with the virtual-user count interpolated between them.
- **REQ-PERF-02 [O]** Each virtual user runs the collection in order. Users can be added or removed
  while a run is in progress. Rows from a data file are handed out in order, at random or round-robin.
- **REQ-PERF-03 [O]** A run can be started, stopped, paused and resumed, and its status is reported as
  it goes.
- **REQ-PERF-04 [O]** Two generations of the execution engine exist side by side. **[I:med]** the newer
  one replaces the older one.
- **REQ-PERF-05 [O]** Metrics: response time and virtual-user count, aggregated overall, per request and
  per virtual user, as summaries and as time series. Errors are grouped by type and by response body,
  with a cap on how many groups are kept.
- **REQ-PERF-06 [O]** When a run uses secrets, the diagnostics it produces keep only timings, sizes,
  status codes and IDs. URLs, names and bodies are omitted.
- **REQ-PERF-07 [O]** Results are written in batches, with retries and a retention policy.
- **REQ-PERF-08 [O]** Results are stored in a local analytical database and queried for summaries,
  failure rates, failing assertions and system metrics.
- **REQ-PERF-09 [O]** CPU and memory are watched during a run, with warning and critical thresholds.
- **REQ-PERF-10 [O]** Traces record the load-profile type and the virtual-user count.

## 8. Agent
- **REQ-AGENT-01 [O]** Request execution runs in a dedicated background worker that shuts down when
  idle.
- **REQ-AGENT-02 [O]** The worker can call back into the app for paths, dialogs, logging, proxy
  resolution and the shell environment. A command-line tool is bundled with the app.
- **REQ-AGENT-03 [O]** The worker runs HTTP, gRPC, WebSocket, Socket.IO and MQTT requests. It can
  install the bundled command-line tool and AI coding-assistant integrations.
- **REQ-AGENT-04 [O]** Requests can be executed by a local desktop agent or by a cloud agent.
- **REQ-AGENT-05 [I:high]** "Agent" means this request-execution layer, which runs in either of the
  two modes above.
- **REQ-AGENT-06 [O]** One message channel carries interactive protocol traffic and vault requests.
- **REQ-AGENT-07 [O]** An optional enterprise access proxy, which uses mutual TLS and issues its own
  certificates.

## 9. Proxy
- **REQ-PROXY-01 [O]** A local proxy that records traffic, installs its own root CA, and can be set as
  the operating system's proxy.
- **REQ-PROXY-02 [O]** Proxy settings can be stored locally or detected from the operating system.
  HTTP and SOCKS proxies are supported, and an auth dialog appears when a proxy asks for credentials.
- **REQ-PROXY-03 [O]** Sign-in for captive portals.
- **REQ-PROXY-04 [O]** Incoming webhooks are forwarded through a tunnel to a local endpoint.

## 10. Mock server
- **REQ-MOCK-01 [O]** A local mock server, configured with examples, scenarios, environment and port.
  If the port is taken it can fall back to another one.
- **REQ-MOCK-02 [O]** A request is matched to a saved example by method, URL, query and body. Request
  headers choose the scenario and the session.
- **REQ-MOCK-03 [O]** Scenarios can simulate rate limits, errors, random failures and added latency.
- **REQ-MOCK-04 [O]** Several mock services can run together, routed by path or by header, with their
  state backed by datasets.
- **REQ-MOCK-05 [U]** How mock examples are authored, and the format of custom handlers.

## 11. Interceptor
- **REQ-INT-01 [O]** A companion browser extension, with a verified native helper, captures browser
  traffic.
- **REQ-INT-02 [O]** The extension and the app talk over an encrypted local channel that checks a key.
- **REQ-INT-03 [U]** The helper's message protocol.

## 12. MCP servers
- **REQ-MCP-01 [O]** Three MCP surfaces exist. One is a development-only server. The second turns an
  API spec or a collection into MCP tools, served on loopback, with a limit on how many run at once.
  The third serves user-defined tools, either scripted or returning static responses, with size limits.
- **REQ-MCP-02 [O]** Several MCP protocol versions are supported.
- **REQ-MCP-03 [O]** The app is also an MCP client, over stdio, HTTP and SSE.
- **REQ-MCP-04 [O]** In-app browser tabs can be driven and recorded as automation scripts.
- **REQ-MCP-05 [U]** What executes the development server's tool calls.

## 13. Datasets
- **REQ-DS-01 [O]** A separate dataset engine runs locally and is reached over an authenticated
  connection. It can resolve vault secrets.
- **REQ-DS-02 [O]** Datasets can be imported from files and specs, queried with SQL, given views, and
  streamed. Connections can be tested, including JDBC sources.
- **REQ-DS-03 [O]** Dataset definition files are written atomically under a file lock.
- **REQ-DS-04 [O]** Query results are columnar, and JDBC sources need a Java runtime.
  **[I:med]** datasets are file-backed.
- **REQ-DS-05 [O]** The performance-results database (REQ-PERF-08) is separate from the dataset engine.

## 14. File system, watcher, search and folders
- **REQ-FS-01 [O]** Workspace file operations: list (also incrementally), read, write, append, delete,
  rename, move, copy and create directories. Every operation reports success or a failure reason.
- **REQ-FS-02 [O]** A file read returns its content and metadata, and errors are reported with a
  message. WSL paths are handled on Windows.
- **REQ-FS-03 [O]** The workspace is watched for created, updated and deleted files. The watcher
  restarts after an error and is cleaned up when its window closes.
- **REQ-FS-04 [O]** Folders inside WSL are watched by polling.
- **REQ-FS-05 [O]** Text and file-name search across the workspace.
- **REQ-FS-06 [O]** A list of recent folders, found from IDE history and from scanning likely
  locations.

## 15. Git integration
- **REQ-GIT-01 [O]** Git built into the app: detect a repository; show the branch, tags and status;
  stage, commit, discard, branch, check out, stash, pull, push, fetch, init and add a remote.
- **REQ-GIT-02 [O]** These operations run the `git` command-line tool, with timeouts and safety
  arguments. Renames are detected, and WSL paths are mapped.
- **REQ-GIT-03 [O]** The app can read a file at a given commit or at HEAD (including in a repository
  with no commits yet). It detects merge-conflict markers (`^(<{7}|>{7})( |$)`) and follows `.git`
  files that redirect to another directory.

## 16. Authentication and secrets
- **REQ-AUTH-01 [O]** Account sign-in happens in the browser and returns to the app through a
  deep-link callback. Sign-up, log in, switch account, log out and skip are all supported.
- **REQ-AUTH-02 [O]** The OAuth 2.0 browser flow captures the authorization code or token from the
  callback, checks TLS, and warns when the connection is plain HTTP.
- **REQ-AUTH-03 [O]** OAuth 2.0 tokens are refreshed automatically. A refresh times out and can be
  cancelled.
- **REQ-AUTH-04 [O]** Local secrets are stored in the operating system's keychain. External vaults are
  supported: Azure Key Vault, AWS Secrets Manager, HashiCorp Vault and 1Password.
- **REQ-AUTH-05 [O]** For each run, secrets are resolved ahead of time and only for allowed domains.
  A secret that is missing fails the run with a configuration error.
- **REQ-AUTH-06 [U]** How auth tokens are stored, and whether vault secrets are cached on disk.

## 17. Migration
- **REQ-MIG-01 [O]** On upgrade, local storage from older versions is migrated, with a progress screen.
- **REQ-MIG-02 [O]** Offline data can be imported into a workspace: collections, environments,
  globals, history, header presets, runs, settings and preferences.
- **REQ-MIG-03 [O]** Legacy account partitions are migrated.
- **REQ-MIG-04 [O]** An offline notice screen with the UI disabled.

## 18. Observability
- **REQ-OBS-01 [O]** Structured logging to the console, to files and optionally to remote sinks.
- **REQ-OBS-02 [O]** Usage analytics and telemetry.
- **REQ-OBS-03 [O]** Crash reporting that can be turned off.
- **REQ-OBS-04 [O]** Diagnostics that do not expose secrets (REQ-PERF-06).
- **REQ-OBS-05 [O]** Tracing.

## 19. Security
- **REQ-SEC-01 [O]** Requests to loopback, private and link-local addresses can be blocked.
- **REQ-SEC-02 [O]** Certificate chains are checked against a denylist of CAs, and client
  certificates are supported (REQ-EXEC-05).
- **REQ-SEC-03 [O]** Scripts, including mock and tool scripts, run sandboxed with restricted file
  access (REQ-EXEC-06, REQ-EXEC-09).
- **REQ-SEC-04 [O]** Secrets are redacted from diagnostics.
- **REQ-SEC-05 [O]** Local channels to companion components are encrypted, and the access proxy uses
  mutual TLS.

## 20. Native capabilities
- **REQ-NATIVE-01 [O]** The app depends on native code for the OS keychain, file watching, a terminal
  (PTY), WebSocket performance, Kafka and an embedded analytical database.

## 21. Non-functional
- **REQ-NFR-01 [O]** Configuration, results, certificates and extensions are kept in the per-user app
  data directory. The working directory lives under the user's home folder.
- **REQ-NFR-02 [O]** Resource limits: concurrent MCP servers, results batch sizes, search output size
  and MCP payload sizes.
- **REQ-NFR-03 [O]** Explicit timeouts and grace periods on background work, token refresh, vault
  auth, tunnels, idle shutdown, polling and file locks.
- **REQ-NFR-04 [O]** Linux, macOS and Windows are supported, including Windows and WSL paths.
- **REQ-NFR-05 [I:high]** Features and release channels are switched on and off at build time.

## 22. Open items
- **[U]** How mock examples are authored; the interceptor helper's protocol; what backs the
  development MCP server.
- **[U]** Whether collection runs can include requests that are not HTTP; how SSE behaves in runs.
- **[U]** Which execution engine is canonical; how auth tokens are stored; whether vault secrets are
  cached.
- **[U]** Where some surfaces are hosted; whether developer tooling ships in production.
