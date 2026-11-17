# PRD — Recreating the Postman Desktop Application

**Subject artifact:** Postman 12.37.1 (Linux x64), Electron app, unpacked JS at `postman-linux-x64/Postman/app/resources/app`
**Purpose of this document:** specify a functional recreation of the observed application. This is a *reverse-engineering-derived* PRD: every requirement is tagged with the strength of the evidence behind it.

## Evidence labeling convention
- **[O]** Observed — directly read in shipped artifact source/config (byte-level or literal string/constant).
- **[I:high|med|low]** Inference — reasoned from observed facts; a stated confidence. Competing explanations noted where material.
- **[U]** Unknown — not established by evidence; must be discovered before it can be specified as a hard requirement.

**Global constraint.** All source is minified/webpack-bundled and was read statically. **No code was executed.** Nothing here claims runtime behavior beyond what static text supports. Absence of evidence is never treated as absence of behavior.

---

## 1. Product overview

A cross-platform desktop API client ("Postman") built on **Electron** (Node 24.14.0 engine, yarn 4). It composes:
1. A **main process** orchestrator (windows, IPC, child processes, native integrations).
2. **Renderer** workspaces (Requester, Scratchpad, Runner, Console, Tester sandbox, Proxy/Auth windows).
3. **Worker/child processes** for request execution, proxies, mock servers, MCP servers, dataset engine.
4. A **request execution engine** (postman-collection / postman-request / postman-runtime / postman-sandbox).
5. **Agent layers** (Desktop/Cloud) for request execution and AI/CLI integration.

**Recreation target:** a behaviorally equivalent application. Original source recovery is out of scope; this PRD specifies *behavior and interfaces*, not source structure.

---

## 2. Architecture requirements

**REQ-ARCH-01 [O]** Multi-process Electron topology: main process + N renderer BrowserWindows + child processes (`child_process.fork`/`spawn`) + `worker_threads.Worker` + `utilityProcess`.
**REQ-ARCH-02 [O]** IPC is the sole cross-boundary transport. Main registers `ipcMain.handle` / `ipcMain.on` dispatch tables; renderer invokes via `ipcRenderer.invoke`/`send`; workers use `process.send`/`postMessage` message-passing RPC.
**REQ-ARCH-03 [O]** A shared `pm` global namespace is exposed in main and (via preload) renderer: `pm.logger`, `pm.sdk.IPC`, `pm.eventBus`, `pm.refreshTokenManager`, `pm.migrator`, `pm.toasts`, `pm.execution`.
**REQ-ARCH-04 [O]** Build-time configuration is injected via `config.json` keys prefixed `__WP_*` (see §3). The recreation must treat these as a typed config surface.

---

## 3. Configuration surface [O]

Observed `config.json` keys (recreation must accept equivalents):

| Key | Role |
|---|---|
| `__WP_SENTRY_DSN__`, `__WP_NEWRELIC_ACCOUNT_ID__`, `__WP_NEWRELIC_INGEST_KEY__` | error/telemetry sinks |
| `__WP_ENABLE_CRASH_REPORTING__` | crash reporting toggle |
| `__WP_RELEASE_CHANNEL__` (`prod`), `__WP_ENV__` (`production`) | channel/env gating |
| `__WP_INTERCEPTOR_APP_ID__`, `__WP_INTERCEPTOR_DEPENDENCIES_DOWNLOAD_URL__`, `__WP_INTERCEPTOR_MANIFEST_FILENAME__`, `__WP_INTERCEPTOR_BRIDGE_SOCKET_NAME__` | Interceptor bridge |
| `__WP_IDENTITY_URL__`, `__WP_GLOBAL_IDENTITY_URL__`, `__WP_AUTHN_URL__` | auth endpoints |
| `__WP_VAULT_AZURE_APP_ID__`, `__WP_VAULT_AZURE_SCOPES__`, `__WP_VAULT_AUTH_REDIRECT_URL__` | Azure Key Vault |
| `__WP_ANALYTICS_URL__`, `__WP_ANALYTICS_CLICK_EVENT_URL__` | analytics |
| `__WP_ALLOWED_PAYMENT_HOSTS__` | Stripe/Google payment hosts |
| `__WP_CAPTIVE_PORTAL_WEB_PROXY_CONFIRMATION_URL_` | web proxy sign-in |
| `__WP_DESKTOP_UI_UPDATE_URL__`, `__WP_GOD_INTEGRATIONS__URL`, `__WP_TUNNEL_LIAISON_ORIGIN__`, `__WP_CLI_INSTALLER_URL__` | service endpoints |
| `__WP_BUILD_SCRATCHPAD__`, `__WP_MODULE_FEDERATION__`, `__WP_IS_PPC_MODE__` | feature gates |

**REQ-CFG-01 [O]** All network endpoints are config-driven, not hardcoded; recreation must parameterize them.
**REQ-CFG-02 [O]** `profile.json` supplies `{name, company, installationOptions}`.

---

## 4. App bootstrap & window lifecycle

**REQ-BOOT-01 [O]** `startup.js` bundles `@postman/app-logger` with collectors: Console, File (winston, 5 files × 5 MB), NewRelic, Sentry.
**REQ-BOOT-02 [O]** `common_services_AppBootListener` fires `AppStatus.appBooted()` on app event `booted` for namespace `requester|scratchpad`.
**REQ-BOOT-03 [O]** `services_LeaderSelection` elects the first "requester" window as leader via `requester-window-events` / `isWindowLeader` / `isWindowLeaderReply`; clears on `window-closed`. Multi-window coordination must follow this leader model.
**REQ-BOOT-04 [O]** `main.js` `HTML_MAP` maps window ids → HTML: `requester`→(workspace), `scratchpad`, `console`, `offline`→`desktop-offline.html`, `no-scratchpad`; dev-urls AUTH/ERROR/PROXY_AUTH/SHELL/WEB_PROXY.
**REQ-BOOT-05 [O]** `services_workingDirManager` creates default working dir `~/Postman/files`.
**REQ-BOOT-06 [O]** `services_devtoolsInstaller` unzips bundled React + MobX DevTools CRX into `userData/extensions` and `session.loadExtension` (chmod 755, 5 retries). **[U]** whether prod-gated.

---

## 5. UI shell

**REQ-UI-01 [O]** Distinct window/page surfaces in `html/`: `requester.html`, `runner.html`, `scratchpad.html`, `no-scratchpad.html`, `console.html`, `tester_sandbox.html`, `webProxy.html`, `proxyAuth.html`, `auth/`, `loader.html` ("Preparing your workspaces..."), `migration-firmware.html`, `file-migration-firmware.html`, `desktop-offline.html`, `shell-migrator.html`.
**REQ-UI-02 [O]** `tester_sandbox.html` is a script-test sandbox exposing globals `responseBody, responseHeaders, responseTime, responseCode, request, iteration, data, postman, environments, globals`, with `Evaluator.js`; `window.open` is blocked.
**REQ-UI-03 [O]** Ten color themes (`js/THEME_*.js`): AYU dark/light, DRACULA, HIGH_CONTRAST dark/light, MONOKAI, NIGHT_OWL dark/light, SOLARIZED dark/light (token names like `base-color-brand`, `content-color-primary`).
**REQ-UI-04 [O]** Preload responsibilities: `paymentPreload` (`electronPayment.close/copyToClipboard`), `tlsDenylistPreload` (SPKI SHA-256 CA denylist, `CERT_SIGNATURE_FAILURE`, `CERT_CHAIN_TOO_LONG`), `visualizer_view_preload` (`pm.getData/getResponse` bridge), `js/preload.js` (`window.SDK_PLATFORM`, `disableScratchpadForSignedIn`, drag/drop blocked).
**REQ-UI-05 [O]** `services_TrackManagementService` IPC `setDesktopUITrack`/`removeDesktopUITrack` selects a client-distribution/remote UI track.
**REQ-UI-06 [I:med]** `shared.html` referenced in scratchpad HTML_MAP but absent; `runner.html`/`loader.html`/`tester_sandbox.html` window creation sites not in `main.js` (likely remote workspace code). Recreation must decide their hosting model.

---

## 6. Request execution core

**REQ-EXEC-01 [O]** Engine stack: postman-collection (v5.x) → postman-request (2.88.x) → postman-runtime (7.x) → postman-sandbox (6.9, over `uvm`).
**REQ-EXEC-02 [O]** Request lifecycle triggers: `beforeItem, item, beforePrerequest, prerequest, beforeTest, test`. Scripts queued as `event {name: prerequest|test, stopOnScriptError, stopOnFailure, abortOnFailure}`. `result.return.nextRequest` implements `setNextRequest`; `pm.execution.runRequest` with max-depth guard.
**REQ-EXEC-03 [O]** HTTP(S) is the runner-transport protocol. Body modes: `raw, urlencoded, formdata, file, graphql`. Diagnostic method whitelist: `GET,HEAD,POST,PUT,PATCH,DELETE,OPTIONS,CONNECT,TRACE`.
**REQ-EXEC-04 [O]** Auth types supported: `noauth, basic, bearer, digest, hawk, oauth1, oauth2, asap, ntlm, apikey, awsv4, edgegrid, jwt`.
**REQ-EXEC-05 [O]** Requester options: `timeout.request`, `timeout.global`, `strictSSL`, `followRedirects`, `maxRedirects`, `followOriginalHttpMethod`, `followAuthorizationHeader`, `disableCookies` (tough-cookie jar), `fileResolver`, client `certificate`. SSE detected via `isSSEStream` (content-type `text/event-stream`).
**REQ-EXEC-06 [O]** Sandbox hardening: `require/eval/console/bridge/__uvm_emit/__uvm_setTimeout` removed from script scope; timeouts `uvm: boot timed out`, `sandbox: synchronous/asynchronous script execution timeout`; marker `"use sandbox2"`.
**REQ-EXEC-07 [O]** `file-resolver.js` resolves request body files: `FileResolver.readFile/createReadStream/stat` route through `_diskPath`; `postman-cloud:///` IDs map via `cloudFiles`; local files gated by `allowLocalFiles` (error *"Local File X is not supported"*); `toNativeLocalPath` normalizes Windows `\/C:/`. Error classifier `isFileLoadFailure` matches `"Form param …, file load error"`, `"Binary file load error"`, `"file data resolution error"`.
**REQ-EXEC-08 [O]** `referenced-request-resolver.js` errors when a run references a missing/deleted request.
**REQ-EXEC-09 [O]** `runtime-platform` provides sandboxed `postmanFs` (`PPERM: insecure file access outside working directory`) and `postman-cloud:///` remote files over `remote-file-ipc-event`.
**REQ-EXEC-10 [I:high]** WebSocket/gRPC/MQTT/Socket.IO support lives in the *interactive* request layer (vendor chunks `postman-ws`, `@postman/runtime grpc-client/grpc-utils`, `mqtt-client`, `socket_io-client`, `websocket-client`, `grpc-reflection-js`, protobufjs), referenced by `scratchpad.js`/`main.js`/`server-methods` — **not** in `execution-worker.js`/`_thread.js`. So: collection **runs** are HTTP(S)-only; the interactive **Send** supports other protocols. **[U]** whether a collection can contain non-HTTP requests at run time.

### 6.1 Error model [O]
Error names surfaced: `TransportError, HttpError, ScriptError, FileLoadError, AssertionError, SecretResolutionError, SecretConfigurationError, WorkerError`. Network: `ETIMEDOUT`, `ESOCKETTIMEDOUT`. Referenced-request: see REQ-EXEC-08.

---

## 7. Collection runner & performance testing

**REQ-PERF-01 [O]** Load profiles: `fixed`, `ramp-up`, `spike`, `peak`, each with required control points (`initial, rampLeadStart, rampLeadEnd, spikeStart, spikePeak, spikeEnd, rampDownStart, rampDownEnd, final`). `evaluateLoadProfile` → transitions; `getTargetVUCount` interpolates VU counts. Sample defaults: fixed 500 VUs/12s, ramp 5→20, spike 2→200→2, peak 4→20→4.
**REQ-PERF-02 [O]** Virtual-user model: VUs execute the collection **sequentially**; dynamic scaling via `ADD_USERS`/`REMOVE_USERS`; data-file row distribution `ordered|random|sequential|round-robin` (`dataRowIndex`, `isRowOutOfBound`).
**REQ-PERF-03 [O]** `ExecutionManagerEvents` = the parent↔handler↔worker command vocabulary: `START, STOP, PAUSE, RESUME, ERROR, ADD_USERS, REMOVE_USERS, GET_PROCESS_USAGE, ASSERTION_METRICS_DATA, SCRIPT_ERROR_DATA, OAUTH_REFRESH_TOKEN_RECEIVED, GET_OAUTH_REFRESH_TOKEN, GET_SYSTEM_PROXY, SYSTEM_PROXY_RECEIVED`. `ExecutorStatus`: `INIT, START, RUNNING, DONE, PAUSED, STOPPING, ERROR, STATUS, DATA, PROCESS_USAGE`. `ExecutorError`: `NO_INSTANCE, CANNOT_START, INVALID_ACTION, CANNOT_REMOVE_USERS, OAUTH_REFRESH_TOKEN_NOT_FOUND`.
**REQ-PERF-04 [O]** Two execution stacks observed (parallel generations):
  - **(A)** `perftest.js`/`perftestExecute.js` → `fork("execution-handler.js")` → `fork("execution-worker.js")` / `new Worker("./execution-worker.js")`; `EXECUTION_HANDLER_GRACE_MS: 7000` then terminate.
  - **(B)** `execution-system-manager` → spawn `ExecutionProcess.js` (type "performance") → per-thread-group `ThreadGroupExecutable.js` (`pid t_group_<id>`) → `new Worker("_thread.js")` per VU.
  **[I:med]** (B) supersedes (A); recreation should implement one coherent engine and decide which is canonical.
**REQ-PERF-05 [O]** Metrics: `metricName: "responseTime" | "virtualUsers"`; `getMetrics → {aggregatedMetrics, groupedByRequest, groupedByVu}`; `metricSummary {summary, timeSeries}`; error segmentation `errors.segmentation.errorTypes[e].segmentation.responseHash` (sha1 body hash, 100-cap, `HASH_COLLISION_DETECTED`, `TOTAL_LIMIT_EXCEEDED`, `OTHER_RESPONSE`).
**REQ-PERF-06 [O]** Diagnostic/redaction policy (`createDiagnosticPolicy`, active when `resolvedSecrets` present): keep `id,timestamp,executionContext,runId,requestId,vuId,vuCount,responseTime,reqSizeBytes,respSizeBytes`, `virtualUser{vuId,dataRowIndex,isRowOutOfBound,dataUsed}`, `response{code,responseTime,responseSize}`; omit URLs/names/bodies as `[OMITTED]`.
**REQ-PERF-07 [O]** Data sink (`PerformanceDataSink`) tables: `perf_request_executions, perf_assertion_results, perf_script_errors, perf_system_metrics, perf_error_response_samples`; fields incl. `response_time_ms, vu_id, vu_count, has_error, http_status_code, req/resp_size_bytes, cpu_percent, memory_used_bytes`; batchSize 1000, flush 2000ms, maxBodyBytes 8192, retries 3 exp-backoff, error-sample dedup by `response_hash`, `data_retention`.
**REQ-PERF-08 [O]** Result analytics are stored in **DuckDB** (`[PerformanceTestV2] DuckDBService`) at `userData/duckdb/results.db` (`:memory:` for history); tables `request_metrics, assertion_details, response_details, script_error_details, system_metrics`. Aggregation query catalog (in `main-performance-test-ipc-handlers`): `summary-metrics, per-request-metrics, test-pass-rate-summary, test-summary-by-request, test-details-by-request, system-metrics-summary/-timeseries, request-failure-rate(-by-request), failed-requests-by-window, error-types-by-request-and-window, top-failed-requests-at-time, top-failed-assertions-at-time, grid-cell-request-breakdown, script-error-messages-by-frequency, requests-with-failed-assertions, response-details`.
**REQ-PERF-09 [O]** System limits: CPU/memory `warning 60 / critical 95`, `consecutiveBreachThreshold 5`, `checkIntervalMs 1000`; device metrics `START/PEAK/BEFORE_END/CURRENT`.
**REQ-PERF-10 [O]** Tracing attributes: `postman.test.profile_type`, `postman.test.virtual_users`.

---

## 8. Agent

**REQ-AGENT-01 [O]** IPC channels: `AGENT_IPC_CHANNEL:"agent2"`, `AGENT_PROCESS_CHANNEL:"agent"`, `AGENT_INTERFACE_CHANNEL:"agent-interface"`, `AGENT_LIFECYCLE_CHANNEL:"agent-lifecycle"`. `AgentWorker.js` runs as a `worker_threads.Worker` with a messageport RPC server (`@postman/runtime.runtime-rpc`); idle shutdown `IDLE_SHUTDOWN_DELAY_MS=3000`.
**REQ-AGENT-02 [O]** Agent interface methods: `app.getPath, os.homeDir, app.getVersion, app.focus, dialog.showSaveDialog, logger.write, proxy.resolve, analytics.sendEvent, app.clearStorageData, platform.execute, accessProxy.getRuntimeState, terminal.getShellEnvironment, terminal.getBundledCli`. Bundled Postman CLI at `resources/data/postman-cli/bin`, env `POSTMAN_CLI_ELECTRON`.
**REQ-AGENT-03 [O]** AgentWorker hosts request execution (`platform.execute`, `grpc-request.invoke`), providers `runtime/platform/terminal/websocket-request/socketio-request/mqtt-request/lifecycle`; forwards fs/proxy/data/aws to main; `PostmanCLIService` (`shell:check-postman-cli`, `postman-cli-install`, PATH shim); `PostmanAgentPluginService` installs "Postman coding agent plugin" via `npx --yes @postman/postman-plugin@latest install --agent claude-code,codex,cursor`.
**REQ-AGENT-04 [O]** Agent identity/network: Cloud Agent gateway WS `__WP_CLOUD_AGENT_GATEWAY_WEBSOCKET_URL__`, service `__WP_CLOUD_AGENT_SERVICE_URL__`, runtime-agent `__WP_RUNTIME_AGENT_SERVICE_URL__`, download `__WP_POSTMAN_AGENT_DOWNLOAD_URL__`. UI strings: "Postman Desktop Agent", "Cloud Agent unavailable", "STDIO transport is disabled on cloud agent. Switch to desktop agent".
**REQ-AGENT-05 [I:high]** "Agent" = request-execution agent layer with two deployment modes (Desktop Agent = separate toolbar app; Cloud Agent = remote network). **[U]** exact `agent2` session-bridging client (Flows/AI builder vs requester).
**REQ-AGENT-06 [O]** `runtime_agents_IPCAgent` (RuntimeIPCAgent): `@postman/runtime.runtime-rpc-electron` Server on ipcMain channel `"@postman/runtime"`; handlers `socketIO-ipc-command/-event/-cb`, `grpc-ipc`, `api-client-ipc`, `vault-ipc`, `vault-ipc-command/-event`, `postman-runtime-ipc-sync`; keytar keychain; `visualizer` session partition.
**REQ-AGENT-07 [O]** `AccessProxyAgent` = `https.Agent` doing TLS CONNECT through an mTLS proxy (retry `TUNNEL_MAX_ATTEMPTS`, CA rotation via managementHost); `AccessProxyHttpBlockAgent` (`HTTP_UNSUPPORTED`); materials `ca.pem/cert.pem/key.pem` in `userData/access-proxy` or `POSTMAN_ACCESS_PROXY_DIR`; `DEFAULT_PROXY_PORT: 8443`, endpoints `/ca.pem`, `/issue-cert`, OID `1.3.6.1.4.1.99999.1`; error *"Enable Secured Access Proxy in Passport settings to provision certificates."*

---

## 9. Proxy

**REQ-PROXY-01 [O]** `ProxyNodeProcess.js` = standalone Node child-process HTTP/HTTPS MITM proxy ("electronProxy", `http-proxy`), openssl-generated root CA `postman-proxy-ca.crt` + `proxy/certificates/cert.json` under `STORE_LOC`; header `x-postman-captr`; media capture filter (audio/video/image); `x-forwarded-*`. Spawned as `proxy_node_process_<release-channel>`, default port **8080**. `enableClientProxy` toggles OS system-proxy; `startProxy` IPC event.
**REQ-PROXY-02 [O]** `CloudProxyManager` = local proxy-config persistence (base122-encoded at `userData/Postman_Config/proxy`) + system-proxy resolution (`session.defaultSession.resolveProxy`) + proxy-auth dialog `proxyAuth.html` (400×320 frameless, partition `proxyConfig`) on Electron `login` (isProxy); IPC `handleProxyAuthSubmit`/`handleProxyAuthCancel`. `getSystemProxy` parses `PROXY|SOCKS host:port` into a ProxyConfig. **[I:med]** naming is a misnomer; no separate "Cloud Proxy" product feature evidenced.
**REQ-PROXY-03 [O]** Web proxy = captive-portal sign-in window (`webProxy.html`) hosting a `<webview>` at `__WP_CAPTIVE_PORTAL_WEB_PROXY_CONFIRMATION_URL_`; closes on redirect to `https://www.tniopdnelrumotsucyxrop.chromiumapp.com`, emits `webproxy-authenticate-close-window-event`.
**REQ-PROXY-04 [O]** Webhook tunnel forwarding: WebSocket to `__WP_TUNNEL_LIAISON_ORIGIN__` at `/ws/<targetId>?token=<metadataToken>`, msgpack framing, `TUNNEL_MESSAGE_VERSION:"0.1.0"`, `HEARTBEAT_INTERVAL_MS:30000`, `TUNNEL_CONNECTION_TIMEOUT_MS:30000`; forwards to local endpoint (host/port/useTLS, loopback allowed); HTTP 401 → `TunnelLiaisonUnauthorizedError`; IPC `webhooks:tunnel:start-forwarding|stop-forwarding|reprovision-required`.

---

## 10. Mock server

**REQ-MOCK-01 [O]** `http-server-worker.js` = child process (`process.send` protocol `start|validate|update-tls|update-pm-data` → `success|error|ready|log|worker-error`). `start` payload: `{handlerPath, port, allowPortFallback, examples, requests, scenarios, honorHeader, environment, globals, overrides}`.
**REQ-MOCK-02 [O]** Matches saved mock examples via `matchRequest` (method + url + query + body). Installs a sandbox `pm` (`pm.test/pm.expect/pm.state`, `pm.mock.matchRequest/sendExample`). Headers `x-mock-scenario`, `x-mock-session`.
**REQ-MOCK-03 [O]** Scenario effects: `rate_limit` (429 + Retry-After), `error`, `chaos` (failure_rate), `latency` (delay_ms).
**REQ-MOCK-04 [O]** `simulation-http-server-worker.js`: `start-simulation` payload `{port, allowPortFallback, routing: path|header, members, enableSession}`; member selection via `x-mock-slug` header or path prefix; dataset-backed state over connectrpc/protobuf + Apache Arrow to the dataset daemon (`sim-bootstrap`, `dataset-operation`); mTLS rotation (`update-tls`). **[I:med]** simulations = multi-member mock environments.
**REQ-MOCK-05 [U]** Mock-example authoring UI/schema and `handlerPath` custom-handler format not inspected.

---

## 11. Interceptor (browser traffic capture)

**REQ-INT-01 [O]** Bridge installer downloads InterceptorBridge (SHA-256 verified) from `__WP_INTERCEPTOR_DEPENDENCIES_DOWNLOAD_URL__`; installs a Chrome native-messaging manifest `{name:"com.postman.postmanapp", type:"stdio", allowed_origins:["chrome-extension://<INTERCEPTOR_APP_ID>/"]}` into OS-native `NativeMessagingHosts/` paths (mac/linux/win).
**REQ-INT-02 [O]** Transport: node-ipc `connectTo("postman")` (`__WP_INTERCEPTOR_BRIDGE_SOCKET_NAME__`), client id `postmanClient`, retry 3000ms, AES-encrypted (key setting `encryptionKeyForInterceptor`, default `postman_default_key`), handshake `KEY_VALIDATION_STRING`; errors `KEY_MISMATCH`, `INVALID_KEY`.
**REQ-INT-03 [U]** InterceptorBridge binary protocol beyond the key handshake (message catalog) is downloaded, not bundled — not readable here.

---

## 12. MCP servers

**REQ-MCP-01 [O]** Three distinct MCP surfaces exist:
  1. **Postman MCP Server** (dev-only): `mcp-server*.js`, `@modelcontextprotocol/sdk` (`server/mcp.js`+`sse.js`), Express on **port 6337** (`/sse` + `/messages`). Tools = payload array `{name, description, parameters}` (JSON Schema), ajv-validated (`strict:false`), schema→zod; each call publishes `{eventId,tool,payload}` to `mcp-data-plane-request`, awaits `mcp-data-plane-response`. Control plane channels `mcp-control-plane-request/response`, cmds `startMCPServer/stopMCPServer`. Initialized only when `"dev"===env`.
  2. **local-mcp-server** (spec/collection → tools): forks `local-mcp-server/worker.js`, serves `http://127.0.0.1:<port>/mcp` (host allowlist `127.0.0.1/localhost/[::1]`). Config: `name, spec XOR collection (≤32MB), externalDocuments, baseUrl, auth (postman{authType: basic|bearer|jwt|oauth1|oauth2}|bearer|basic|apiKey in header/query), variables (environment/collection/globals/vault `vault:`+domains), toolNames/toolDescriptions/excludedTools/unavailableVariables/toolArguments (query|header|body)`. Max **4** concurrent. IPC `local-mcp-server:start|stop|preview|list|get`; worker msgs `ready/started/start-error/previewed/runtime-error/call-started/call-finished/stopped/log`. Tool names = naming fn | operationId | `<method> <path>` (64-char cap + hash suffix); `collection-toolset.ts` turns collection requests into tools (with `{{var}}` substitution), executed via postman-runtime fetch.
  3. **toolset-mcp-server** (user-defined tools): validates snapshot `{fingerprint, name, tools≤100: name≤128, description≤8KB, inputSchema≤64KB, implementation: {type:"script", language:"javascript", code≤256KB} | {type:"static", response:{type:"text"|"json"}}}`. IPC `toolset-mcp-server:start|apply|stop|get|get-call-payload` (payloads ≤256KB/req, 64KB preview). ajv per-tool validators; static returns text/json; script runs in postman-sandbox.
**REQ-MCP-02 [O]** Both workers speak `@modelcontextprotocol/sdk` with protocol versions `["2025-11-25","2025-06-18","2025-03-26","2024-11-05","2024-10-07"]` (latest `2025-11-25`).
**REQ-MCP-03 [O]** `AgentWorker.js` is also an MCP **client** (Stdio / StreamableHTTP / SSE client transports, `callTool`/`listTools`).
**REQ-MCP-04 [O]** Browser tooling: `main.js` defines toolset `postman-agent-mode` v1.0.0 with tools `browser_navigate/click/type/fill_form/press_key/select_option/snapshot/wait_for/handle_dialog/navigate_back/evaluate`, emitting recorded code (`await page.goto(...)`), driven by the Playwright-derived `browser_tab_recorder_preload.js` (`InjectedScript`, `__pw_recorderState`, `codegen`/`autoexpect` locator modes, aria snapshots) over `BrowserTabViewManager` in-app tabs (WebContentsView, partition `persist:browser-tab`, IPC `browser-tab:view:nav-state`).
**REQ-MCP-05 [U]** What executes `mcp-data-plane-request` payloads (the dev MCP server's tool backend) was not found.

---

## 13. Datasets

**REQ-DS-01 [O]** `dataset-daemon-service` spawns external binary `postman-dataset-engine[-<os>-<arch>][.exe]` (`--config`), endpoint `tcp://127.0.0.1:<freePort>` (win32) else unix socket `tmpdir/pmde-<hex>.sock`; gRPC client `@postman/dataset-engine-sdk/dist/client/grpc_js` with mTLS (`DATASET_ENGINE_MTLS_BUNDLE`, tls-rotate listeners). Renderer resolves vault secrets via `dataset-secret-resolver-ready` / `dataset-resolve-secrets-request|response`.
**REQ-DS-02 [O]** Dataset IPC (registered in `main.js`): `dataset-daemon-ensure-running|status|health-check|list-datasets|get-dataset-full|import-from-file{filePath,dataDir}|import-from-spec|delete-dataset|query{datasetId,sql,params}|execute-native-query{datasourceRef}|add-view|remove-view|execute-view|view-row-count|describe-source{path,format}|reingest-datasource|test-connection|test-jdbc-connection|detect-java|inspect-jdbc-driver|jdbc-driver-properties|set-jdbc-runtime-preference|open-stream|pull-stream|cancel-stream`, plus `dataset-yaml-locked-write`. Streaming via batched `pullDatasetStream({streamId,batchSize})`.
**REQ-DS-03 [O]** `dataset-file-lock` (`withFileLock` from dataset-engine-sdk): writes `<file>.lock` (pid, flag `wx`), stale after 10s, 5s max wait, 50ms poll; used for atomic dataset-**YAML** writes (`views:` sections; sdk `dist/pm/dataset/create|handle`, `state/create`; `redactCredentials.js`).
**REQ-DS-04 [O]** Engine wire surface is Arrow-based (`QueryArrow`, `ViewArrow`, `ExecuteView`, `ViewRowCount`, `ImportPaths`, `ViewService`, `HealthService`); sources include files/specs and **JDBC** (via detected Java). **[I:med]** datasets = file-backed (YAML) dataset manager over a separate gRPC ingestion/SQL/view engine. **[U]** whether the engine embeds DuckDB.
**REQ-DS-05 [O]** **DuckDB is NOT the dataset engine** — it is the performance-test result store (see REQ-PERF-08).

---

## 14. File system, watcher, search, folders

**REQ-FS-01 [O]** File-system IPC (`fs:*`): `loadDirectory, getAllDirectoryItems, loadDirectoryProgressive, indexDirectoryStart/Next/Cancel, readFile, readFileBase64, writeFile, writeFileBase64, appendFile, deleteFiles, existsPath, renameFile, movePath, copyFiles, createDirectory` → `FileSystemService`. Handlers return `{success, items|error}`.
**REQ-FS-02 [O]** `FileSystemService.readFile(path, windowId)`: `resolveValidatedPath` (custom `validatePath` else `path.resolve`) → `_readFileWithWslFallback(path, opts, "utf-8")` ( `fs.readFile` + `fs.stat({bigint:true})`; on failure `_resolveWslSymlinkReadTarget` then re-read) → `{success:true, content, lastModified, size, ino}` or `{success:false, error:"Failed to read file: …", code}`. `readFileBase64` returns `content.toString("base64")`.
**REQ-FS-03 [O]** Watcher (`main-file-system-watcher-*`) → `@parcel/watcher` (watchman-preferred). Out events: `fs:directory:watch:events` `{watcherIds, dirPath, events:[{type:"create"|"update"|"delete", path}]}`, `fs:directory:watch:error`; in `fs:watchDirectory`, `fs:unwatchDirectory`. Auto-restart ≤5, per-webContents cleanup.
**REQ-FS-04 [O]** WSL UNC paths (`\\wsl.localhost|\\wsl$\…`) skip parcel: `WslPollWorker.js` forked via `utilityProcess` (`serviceName:"wsl-poll-watcher"`), polls `wsl.exe -d <distro> -e env LC_ALL=C find … -printf '%y\t%T@\t%s\t%p\0'` every 2s, diffs mtime/size baseline to synthesize create/update/delete.
**REQ-FS-05 [O]** Ripgrep search (`main-ripgrep-ipc-handlers`, `@vscode/ripgrep`): channel `ripgrep-search` → `search({queryString, queryPatterns, fileNamePatterns}, dir)` runs `rg --no-config [-g glob]… [-F -e queryString] [-e pattern]… -- <dir>` (or `--files` when only globs); `execFile` maxBuffer 20MB.
**REQ-FS-06 [O]** Recent folders (`main-recent-folders-ipc-handlers`): discovery from `ide-recents` (VS Code recents, `node:sqlite`) + `scan` (devRootScan, gitWorktreeProbe); priority `{ide-recents:1, scan:2}`.

---

## 15. Git integration

**REQ-GIT-01 [O]** `main-git-ipc-handlers` registers **43 `git:*` channels**: `detectFolderGit, isGitAvailable, isWorkingDirectoryClean, getOriginUrl, getCurrentBranch, getDefaultBranch, getCurrentTag, getTags, getLocalBranches, getRemoteBranches, getLogAuthors, getRelativePath, getChangedFiles, getStagedFiles, getUnstagedFiles, getAheadBehind, getOutgoingCommits, showFileAtRef, showFileAtHead, findDeletingCommits, findPathsInLatestCommit, diffWorkingTreeRenames, diffCommitRenames, discardPaths, commitPaths, commitAll, commitFile, commitFileOnBranch, createBranch, deleteBranch, checkoutBranch, checkoutRemoteBranch, checkoutTag, stashChanges, restoreStash, getStashBlocker, stageUntrackedFromBranch, postmanResourcesExistAtRef, pull, push, fetch, init, addRemote`.
**REQ-GIT-02 [O]** Git commands observed: `rev-parse, for-each-ref, checkout, stash, status, reflog, log, config, update-ref, symbolic-ref, --version`. Rename/diff helpers + timeouts from `@postman/linter/dist/cjs/rename/gitProbe.js` (`collectCommitDiffs, renameDiffArgs, GIT_*_TIMEOUT_MS, GIT_SAFETY_ARGS, MAX_DIFF_BUFFER_BYTES`). `folderGitProbe.js` hand-parses `.git/config` incl. WSL path mapping (`WslPath.js`); telemetry `GitDetectionOutcome`.
**REQ-GIT-03 [O]** `FileSystemService` git content reads: `readFileAtCommit(repo, commit, path)` (`git rev-parse --verify <commit>^{commit}` then read blob), `showFileAtHead` (commit `"HEAD"`, handles unborn head), `hasConflictMarkers` (`fs.promises.readFile(path,"utf8")`, regex `^(<{7}|>{7})( |$)`), `resolveGitDirectory` (reads `.git` for `gitdir:` redirect). A CI-file reader is gated by an allowed-path list. The service exposes an endpoint named `git` (mechanism `property:path`).

---

## 16. Authentication & secrets

**REQ-AUTH-01 [O]** Sign-in is **web-redirect** based. `js/authentication.js` "AuthenticationWindow": appId `erisedstraehruoytubecafruoytonwohsi`, redirect `https://…chromiumapp.org`, partition `"authentication"`. Flows: `identityURL` (identity.getpostman.com `/client/login`, `/client/browser-auth/init` targeting `switch-context, google, enterprise-login, github`), `globalIdentityURL` (auth.postman.com `/__redirect/client/signup` with `__showRegionChooserIfNoneDetected`/`__region`). Deep link `postman://auth/callback`. `main.js` `AUTH_ACTIONS = signup|login|switch|logout|skip`.
**REQ-AUTH-02 [O]** Collection OAuth2: `OAuth2WindowManager` 800×600 "Postman OAuth 2.0" window (contextIsolation, cookie partition), matches callback URL, intercepts redirects 301/302/303/307/308 + `onHeadersReceived`, extracts `code`/`access_token` from query+hash per `t.params`, TLS-denylist cert checks, warns "(insecure connection)" on http; `pm.eventBus` channels `oauth2-login`, `oauth2BrowserLogin`/`startLogin`/`loginCallback`.
**REQ-AUTH-03 [O]** Token refresh: `AppRefreshTokenManager.refreshToken(authSessionId, cb)` publishes `oauth2-refresh-token` on `oauth2-ipc-event`, 30s timeout, resolves `oauth2-token-refreshed` (accessToken), cancels `oauth2-cancel-refresh-token`.
**REQ-AUTH-04 [O]** Secret storage — local: keytar (`common/services/CryptoService.js`: `storeInKeychain`/`retrieveFromKeychain`, service `${app.getName()} (${id})`, account `userKey`). External vaults (`VaultIntegrationService.VaultManager.createVault`) for `azure-key-vault | aws-secrets-manager | hashicorp-vault | one-password`:
  - Azure: PKCE, authority `https://login.microsoftonline.com/common`, timeout 300s.
  - 1Password: `@1password/sdk` `createClient({auth: serviceToken})` (token starts `ops_`), `client.secrets.resolve(secretId)`.
  - HashiCorp: OIDC discoveryUrl.
  - AWS: STS AssumeRole `roleSessionName:"postman-vault"`.
**REQ-AUTH-05 [O]** Per-run secret/package resolution: `secret-resolver-from-map.js` `createSecretResolverFromMap(resolvedSecrets)` maps `variable.source.postman.secretId`→prefetched value, domain-gated (`UrlMatchPattern`, `isUrlAllowedForDomains`), error `SecretConfigurationError` ("Secret not found in pre-fetched vault data"); `prepareSecretResolver` validates `variable.secret` across environment/globals/collection/referencedRequests. `package-resolver-from-map.js` resolves `{packages:{name:{id}}}` (error "Package … was not resolved for this performance run"). Consumed by `execution-worker.js`.
**REQ-AUTH-06 [U]** Exact auth token persistence format post-keychain; role of `__WP_AUTHN_URL__`; whether vault secrets are cached to disk (`secretCacheTtl` seen, medium unconfirmed).

---

## 17. Migration firmware

**REQ-MIG-01 [O]** `file-migration-firmware`: moves legacy `file://`-origin localStorage/IndexedDB/settings into the new https-origin partition. Constants `fileToHttpsIndexeddbMigration, fileToHttpsSettingsMigration, indexeddbMigrationFailedModels`. `bootFileProtocolMigrationFirmware` loads `html/file-migration-firmware.html` in a hidden webview (`id="file-protocol-migrator"`, nodeIntegration, `pathToFileURL`, persisted partition). UI = "Updating Postman…" + indeterminate ProgressBar.
**REQ-MIG-02 [O]** Scratchpad→cloud import (`RequesterModalContainer` `SetupWorkspace~importCollections`): imports `pm.migrator.models` (persistent-nedb) collections, environments, globals, history, header presets, collection runs, settings, preferences into a workspace (max 1000 collections, `uid=${userId}-${id}`).
**REQ-MIG-03 [O]** `shell-migrator.js` replies to `shellMessage`/`getShellData` with `localStorage`; `main.js` `services/shellDataMigration.performShellDataMigration` migrates keys `v8PartitionsNamespaceMeta, partitions, v8Partitions, users, activePartition, lastActiveUser, isPartitionDirty` (legacy auth partitions/users).
**REQ-MIG-04 [O]** `desktop-offline.js`: offline view invoking `GET_OFFLINE_DATA`, renders `pm.offlineData.userName/email` with disabled UI.

---

## 18. Observability

**REQ-OBS-01 [O]** Structured logging via `@postman/app-logger` (winston) with File/NewRelic/Sentry/Console collectors (§4).
**REQ-OBS-02 [O]** Telemetry: `analytics.sendEvent` / `__WP_ANALYTICS_URL__` / `__WP_ANALYTICS_CLICK_EVENT_URL__`; `telemetry` Git detection outcome; tracing attrs (REQ-PERF-10).
**REQ-OBS-03 [O]** Crash reporting gated by `__WP_ENABLE_CRASH_REPORTING__`.
**REQ-OBS-04 [O]** `diagnostic-summary.js` `createDiagnosticPolicy` implements secret-safe diagnostics (REQ-PERF-06).
**REQ-OBS-05 [O]** `utils_enableTracing` enables tracing; `tracing/` dir present.

---

## 19. Security requirements

**REQ-SEC-01 [O]** `restricted-addresses.js` blocks localhost/loopback/private CIDRs (`10/8, 172.16/12, 192.168/16, 169.254/16, fc00::/7, …`), code `ERR_RESTRICTED_ADDRESS`; runner emits `NETERR: getaddrinfo ENOTFOUND <host>` when `isAddressRestricted`. `UnifiedRuntimeService.assertSourceAllowed` → `Address is blocked`/`IP_BLOCKED` via `dns.blockList`.
**REQ-SEC-02 [O]** TLS: `tlsDenylistPreload` rejects chains whose SPKI SHA-256 is denylisted (named CAs, max chain 16, `checkServerIdentity` override). Client certificates supported on requests (REQ-EXEC-05).
**REQ-SEC-03 [O]** Sandbox: postman-sandbox/uvm with scope stripping (REQ-EXEC-06); `postmanFs` permission boundary (REQ-EXEC-09); mock/toolset code runs in postman-sandbox.
**REQ-SEC-04 [O]** Secret redaction in diagnostics (REQ-PERF-06); `redactCredentials.js` in dataset SDK.
**REQ-SEC-05 [O]** Interceptor bridge is AES-encrypted node-ipc with key validation (REQ-INT-02). Access Proxy uses mTLS cert provisioning (REQ-AGENT-07).

---

## 20. Native addons [O]

| Addon | Purpose |
|---|---|
| `watcher.node` | `@parcel/watcher` (fs watcher backend) |
| `keytar.node` | OS keychain (CryptoService) |
| `pty.node` | `@postman/node-pty` terminal / agent shell |
| `node.napi{,1,.glibc,.glibc1}.node` | shared prebuilds: bufferutil / utf-8-validate (ws perf) + confluent kafka bindings |
| `core_bg.wasm` | 1Password SDK |
| `duckdb.node/.so/.dylib/.dll` | DuckDB (perf results) |

**REQ-NATIVE-01 [O]** Recreation must provide equivalents for the keychain, file-watcher, PTY, and DuckDB capabilities, or substitute with documented adapters.

---

## 21. Non-functional requirements

**REQ-NFR-01 [O]** Persistence: `userData` for config, proxy, DuckDB results, extensions, access-proxy certs; `~/Postman/files` default working dir.
**REQ-NFR-02 [O]** Concurrency caps: local-mcp-server ≤4 instances; perf data-sink batchSize 1000 / flush 2s; ripgrep maxBuffer 20MB; MCP tool payload caps (256KB code, 64KB schema, 100 tools).
**REQ-NFR-03 [O]** Timeouts/grace: execution-handler grace 7000ms; OAuth2 refresh 30s; Azure vault 300s; tunnel heartbeat 30s; agent idle shutdown 3s; watcher poll 2s; file-lock stale 10s.
**REQ-NFR-04 [O]** Cross-platform: Linux/macOS/Windows path & native-messaging handling (`toNativeLocalPath`, WSL path mapping, OS-specific NativeMessagingHosts).
**REQ-NFR-05 [I:high]** Modular webpack-bundle layout with `__WP_*` config injection enables channel/feature gating (`prod`, `dev`, `scratchpad`, `PPC`).

---

## 22. Out of scope / open items (must resolve before hard sign-off)

- **[U]** Exact renderer→main invocation site for `fs:readFile` (recorded as residual unknown `unk_8fc6ed93…`).
- **[U]** InterceptorBridge binary message catalog (downloaded, not bundled).
- **[U]** Mock-example authoring UI/schema and `handlerPath` format.
- **[U]** Dev MCP server `mcp-data-plane-request` backend executor.
- **[U]** Whether collection runs can include non-HTTP requests; SSE assertion/UX behavior.
- **[U]** Whether `ExecutionProcess`/`ThreadGroupExecutable` supersedes or wraps `execution-worker` (pick one canonical engine).
- **[U]** Auth token persistence format; vault secret disk caching.
- **[U]** `shared.html` purpose; window-creation sites for `runner`/`loader`/`tester_sandbox`.
- **[U]** Whether :6337 SSE MCP server and CRX devtools install are prod-gated.
- **[U]** Simulation chaos/condition DSL and dataset daemon wire protocol details.

---

## 23. Suggested build order (recreation)

1. Electron shell + config + bootstrap + windows + logging (§2–5, §18).
2. Request execution core + sandbox + file-resolver (§6) — the foundation.
3. File system / watcher / ripgrep / git (§14–15).
4. Auth + secrets (§16).
5. Collection runner + performance testing + DuckDB analytics (§7).
6. Agent + Proxy + Mock server (§8–10).
7. MCP servers (§12) + Datasets (§13).
8. Interceptor + browser tabs + visualizer + migration (§11, §17).
9. Security hardening + native addons + NFR tuning (§19–21).

Each step should be verified against the observed behavior before proceeding; §22 unknowns gate final equivalence claims.
