# ADR-0003: Timing phases are measured around reqwest's futures; DNS/connect/TLS stay None (D2)

- **Status: Accepted**
- **Date:** 2026-11-19
- **Source:** plan §4.D2 (PRD §8.2 REQ-NET-01/02, §8.2 spike note)

## Context

REQ-NET-01 wants the phases of a send — prepare/socket wait, DNS lookup, TCP connect, TLS handshake,
time to first byte, download, total — and says outright that phases which cannot be measured are
omitted, **never estimated**. REQ-NET-02 wants network and TLS facts (addresses, ALPN, protocol,
cipher, certificate chain, verification flag). `ReqwestTransport` sends through `reqwest` 0.13.5 on
the rustls backend, so the S12 spike had to establish what that version actually exposes, and
whether the transport needs a hyper-level connector to answer either requirement.

Findings, verified against the pinned sources of `reqwest` 0.13.5 and `hyper-util` 0.1.21 (and the
upstream docs):

- **No timing data at all.** `Response`, `RequestBuilder` and `Client` expose no phase timings; no
  hook reports when DNS resolution, the TCP connect or the TLS handshake started or finished.
- **The connect call is opaque and cannot be replaced.** `ClientBuilder::connector_layer` wraps the
  whole of it in one tower layer, but the request and response types of that service
  (`reqwest::connect::sealed::Unnameable`, `...::Conn`) are sealed and unnameable from outside, so
  only generic layers fit — and they see DNS + TCP + TLS as one indivisible future. A connector that
  separates them means `reqwest` is not the client any more: `hyper_util::client::legacy::Client`
  plus a hand-written DNS/TCP/TLS connector, and with it request building, response decompression
  (gzip/brotli/deflate), error classification and the TLS-verification toggle all get reimplemented.
- **What is observable** on a response: `remote_addr()` (the remote `SocketAddr`, read from the
  `hyper_util::client::legacy::connect::HttpInfo` extension, which also carries `local_addr()`), and
  — with `ClientBuilder::tls_info(true)` — `reqwest::tls::TlsInfo`, which reports `version()`
  (TLS 1.0–1.3) and `peer_certificate()`, the DER **leaf** certificate only (reqwest keeps
  `peer_certificates().first()` and drops the rest of the chain).
- **Cipher suite and ALPN are not exposed anywhere.** The ALPN marker on hyper's `Connected` is
  internal bookkeeping and never reaches response extensions.

## Decision

**No hyper connector.** `ReqwestTransport` keeps `reqwest::Client`, and `Timings` is filled only with
phases the transport measures itself, around reqwest's own futures (the clock starts when the
request is handed to reqwest, after the body bytes are in hand):

- `first_byte` — **measured**: request handed over → response head arrived.
- `download` — **measured**: head arrived → body read in full.
- `total` — **measured**: the whole of those two.
- `prepare`, `dns`, `connect`, `tls` — **`None`**. reqwest gives no way to observe them and they are
  never estimated: a `Some` here would be a number nobody measured.

`NetworkInfo` follows the same rule: remote IP and port from `remote_addr()`, the local socket
address from `HttpInfo`, the TLS protocol version and the send's `verify_tls` flag from
`reqwest::tls::TlsInfo` and `SendSettings`. ALPN, the cipher suite and the certificate chain stay
`None` — reqwest does not report them.

## Consequences

- S31's Timeline renders bars for the three measured phases and lists the four unmeasurable ones
  from this ADR: "absent rather than zeroed" (REQ-NET-01).
- REQ-NET-02's ALPN, cipher and certificate chain (subject, issuer, validity, SANs, SHA-256
  fingerprint) stay unfulfilled. **CARRY-FORWARD:** filling them needs the hyper-level connector
  inside `ReqwestTransport` and nowhere else (PRD §8.2), or — for the chain alone — an X.509 parser
  dependency to turn the DER leaf reqwest does give into the fields REQ-NET-02 names.
- `TlsInfo`'s protocol, cipher and certificates are `Option`: a transport that cannot report a fact
  leaves it `None`, so the response viewer says "not reported" instead of showing a value nobody
  observed.
- A send on a reused connection reports a `first_byte` that covers no connect at all. That is
  honest; the "reused connection, 0 ms connect" note of REQ-NET-01 needs the connector work too.

## Dissent / alternatives rejected

- **A hyper connector inside `ReqwestTransport`** — rejected for this step: reqwest cannot host one
  (sealed `Conn`/`Unnameable`), so choosing it means replacing the client wholesale, with
  decompression, error mapping and the verification toggle rebuilt on top. That is follow-up work,
  not a spike deliverable.
- **Deriving DNS/connect/TLS by subtraction or proportion** — rejected: REQ-NET-01 forbids
  estimating a phase, and a number that was not measured is not a measurement.
- **Inferring ALPN from the negotiated HTTP version** — rejected: `http2_prior_knowledge` is a
  request setting, so an HTTP/2 response proves nothing about what ALPN said.
- **Hand-parsing the DER leaf into `Certificate` fields** — rejected: it needs an X.509 parser and
  SHA-256, i.e. new dependencies for half a requirement, while the chain stays unobservable either
  way.
