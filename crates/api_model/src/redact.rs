//! Shared redaction (D8): the single place Pustman masks a secret. History, the Console and
//! every export run their text, headers and URLs through here before anything is stored,
//! shown or written out (REQ-NFR-04, REQ-HIST-03, REQ-CON-05, REQ-NET-04).
//!
//! Pure string work: no IO, no logging, no clock. Nothing in this module may print or keep
//! a value — it is the masking point, so a leak here would leak everywhere.

use std::collections::BTreeSet;
use std::fmt;

/// Every redacted occurrence becomes this. A fixed shape hides how long the secret was, and
/// the bullet reads as "something was here" in any font.
const MASK: &str = "••••••";

/// The one character [`MASK`] is built from. The invariant is that no secret contains this
/// character at all — not merely that no secret is made only of it. A secret holding even
/// one bullet can match across the junction of kept text and a mask that an earlier pass
/// wrote: `ab••` turns `abab••` into `ab••••••`, which the next pass turns into
/// `••••••••••`. Redaction must be idempotent, so any such value is refused, and the refusal
/// is reported (see [`SecretValues::refused_count`]).
const MASK_CHAR: char = '•';

/// Fewer characters than this is ordinary text, not a secret: masking "id" or "12" would
/// destroy every line it touches while protecting nothing worth protecting.
const MIN_SECRET_CHARS: usize = 4;

/// The headers whose value is a credential whatever it contains. The first four are the S3
/// mandate; the others carry a credential by the same definition — an API key, a session
/// token, a CSRF token or a signature is no less a credential for living in a vendor header
/// (REQ-HIST-03: "Authorization, cookies, secret variable values, OAuth tokens").
const CREDENTIAL_HEADERS: [&str; 14] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "api-key",
    "x-auth-token",
    "x-access-token",
    "x-csrf-token",
    "x-amz-security-token",
    "x-hub-signature",
    "x-hub-signature-256",
    "www-authenticate",
    "proxy-authenticate",
];

/// The URI schemes that carry no authority (RFC 3986 §3.3): everything after the `:` is an
/// opaque path, so `mailto:bob@example.com` has no userinfo to mask. Only consulted for URLs
/// without `//`, where a scheme and the schemeless `user:pass@host/…` shape are otherwise
/// indistinguishable.
const OPAQUE_SCHEMES: [&str; 16] = [
    "mailto",
    "tel",
    "sms",
    "smsto",
    "geo",
    "urn",
    "news",
    "nntp",
    "sip",
    "sips",
    "xmpp",
    "im",
    "irc",
    "ircs",
    "data",
    "javascript",
];

/// The literal secret values that must never survive into stored or exported text.
///
/// Values shorter than four characters and values that contain the mask character cannot be
/// masked and are refused; see [`MIN_SECRET_CHARS`] and [`MASK_CHAR`]. A refused value is
/// never masked anywhere, so refusal is not silent: [`SecretValues::insert`] returns `false`
/// and [`SecretValues::refused_count`] counts them, and the caller must warn the user that
/// those values will appear unmasked. `Debug` prints counts only: a set of secrets must not
/// leak the very values it exists to hide.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValues {
    values: BTreeSet<String>,
    refused: usize,
}

impl SecretValues {
    /// An empty set: only the always-masked headers and URL userinfo are redacted.
    pub fn new() -> Self {
        Self {
            values: BTreeSet::new(),
            refused: 0,
        }
    }

    /// Collect `values`, keeping those that can be masked; check [`Self::refused_count`]
    /// afterwards and warn the user about the rest.
    pub fn from_values<I, S>(values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut secrets = Self::new();
        for value in values {
            secrets.keep(value.into());
        }
        secrets
    }

    /// Add one value. Returns `false` when the value cannot be masked (too short, or holding
    /// the mask character): it is then counted in [`Self::refused_count`] and will appear
    /// unmasked, which the caller must tell the user. An empty value is nothing to hide and
    /// is ignored without being counted.
    #[must_use = "a refused secret will appear unmasked; the user must be told"]
    pub fn insert(&mut self, value: impl Into<String>) -> bool {
        self.keep(value.into())
    }

    /// [`Self::insert`] without `must_use`: [`Self::from_values`] reports refusals through
    /// [`Self::refused_count`] instead of per value.
    fn keep(&mut self, value: String) -> bool {
        if is_maskable(&value) {
            self.values.insert(value);
            return true;
        }
        if !value.is_empty() {
            self.refused += 1;
        }
        false
    }

    /// How many non-empty values were refused because they cannot be masked.
    pub fn refused_count(&self) -> usize {
        self.refused
    }

    /// How many values are kept.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether nothing is kept.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl Default for SecretValues {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SecretValues {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretValues")
            .field("len", &self.values.len())
            .field("refused", &self.refused)
            .finish()
    }
}

/// Replace every occurrence of every secret in `text` with the mask.
///
/// Occurrences of different secrets may overlap — one secret can contain or straddle
/// another. Every overlapped span is masked as a whole, so replacing one secret never
/// leaves a fragment of another in the clear. Running the result through here again returns
/// it unchanged: the mask carries no secret (values containing its character are refused,
/// see [`SecretValues`]), so an already-redacted string never matches again.
pub fn redact_text(text: &str, secrets: &SecretValues) -> String {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for secret in &secrets.values {
        push_spans(text, secret, &mut spans);
    }
    if spans.is_empty() {
        return text.to_owned();
    }
    spans.sort_unstable();
    let mut redacted = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut span = spans[0];
    for &(start, end) in &spans[1..] {
        if start < span.1 {
            span.1 = span.1.max(end);
        } else {
            redacted.push_str(&text[cursor..span.0]);
            redacted.push_str(MASK);
            cursor = span.1;
            span = (start, end);
        }
    }
    redacted.push_str(&text[cursor..span.0]);
    redacted.push_str(MASK);
    redacted.push_str(&text[span.1..]);
    redacted
}

/// Every occurrence of `secret` in `text`, as byte spans, including self-overlapping ones
/// ("aaaa" occurs twice in "aaaaaa").
fn push_spans(text: &str, secret: &str, spans: &mut Vec<(usize, usize)>) {
    let mut from = 0;
    while let Some(offset) = text.get(from..).and_then(|rest| rest.find(secret)) {
        let start = from + offset;
        spans.push((start, start + secret.len()));
        from = start + text[start..].chars().next().map_or(1, char::len_utf8);
    }
}

/// Redact one request's headers: the credential headers carry a credential by definition and
/// are masked in full whatever the caller knows, and every other value still has
/// [`redact_text`] applied. Header names are matched case-insensitively on their visible
/// ASCII, because HTTP header names are case-insensitive ASCII tokens (RFC 9110 §5.1) and a
/// padded name is still that header. Names run through [`redact_text`] too, so a secret in a
/// name cannot survive.
pub fn redact_headers(
    headers: &[(String, String)],
    secrets: &SecretValues,
) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| {
            // Decide on both forms: a secret glued into the name changes it on the first
            // pass, and the decision must not differ between the first pass and the next.
            let redacted_name = redact_text(name, secrets);
            let value = if is_credential_header(name) || is_credential_header(&redacted_name) {
                MASK.to_owned()
            } else {
                redact_text(value, secrets)
            };
            (redacted_name, value)
        })
        .collect()
}

/// Redact a URL: the userinfo is masked whatever the caller knows, and any secret in the URL
/// is masked too.
///
/// A component is masked whole when its decoded form hides a secret: `p%40ss` and `p+ss`
/// name the same bytes as `p@ss` and `p ss`, and decoded text cannot be mapped back onto
/// the encoded bytes to replace only the secret inside it. That covers query values, query
/// names, path segments, the fragment and the userinfo. Everything else is redacted
/// occurrence by occurrence like [`redact_text`].
///
/// A secret that holds a delimiter (`/ & = ? #`) can straddle components and, with one byte
/// percent-encoded, appear in no single component nor in the raw text. A last pass over the
/// whole decoded URL catches it and masks every component it touches; see
/// [`mask_decoded_spans`].
///
/// Userinfo: `user:pass` keeps its username and masks the password, and a username-only
/// userinfo is masked whole because it is the credential itself. Scheme-relative
/// (`//user:pass@host/…`) and schemeless (`user:pass@host/…`) URLs are parsed for userinfo
/// too.
pub fn redact_url(url: &str, secrets: &SecretValues) -> String {
    // Masking can cut a percent escape or join text across a mask, so one pass is not
    // guaranteed to settle; repeat until it does. The loop ends because masking only ever
    // replaces text with the mask and no secret contains the mask character, so a pass that
    // changes the URL either consumes at least one non-mask character or moves a bullet run
    // in a password closer to exactly the mask's length (a short run grows to it, a long run
    // shrinks to it). Both measures move monotonically down — non-mask character count, and
    // |bullet-run length − mask length| — and a pass that consumes no non-mask character
    // cannot change the parse, so the next pass is a no-op. The result is a fixed point,
    // which is what idempotence needs. An empty password is not expanded to the mask: it has
    // nothing to hide, and inserting bullets there would only assert a password exists.
    let mut current = url.to_owned();
    loop {
        let next = redact_url_once(&current, secrets);
        if next == current {
            return next;
        }
        current = next;
    }
}

fn redact_url_once(url: &str, secrets: &SecretValues) -> String {
    // Registered-secret occurrences and userinfo password regions are computed against the
    // SAME input and masked as one union. Running `redact_text` first would consume the `@`
    // of a secret that straddles it and leave the rest of the password in the clear; running
    // `mask_userinfo` first would overwrite part of a secret that contains its own `://` and
    // leave the rest of the secret in the clear. Neither heuristic may preempt the other.
    let masked = mask_url_spans(url, secrets);
    let parts_masked = mask_encoded_parts(&masked, secrets);
    let text_masked = redact_text(&parts_masked, secrets);
    mask_decoded_spans(&text_masked, secrets)
}

/// Mask every registered-secret occurrence and every userinfo password region of `url`, as
/// one union of byte spans. A username prefix is kept only by falling in a gap: when a secret
/// touches it, the union covers it and it is masked like the rest. Gaps are emitted verbatim.
fn mask_url_spans(url: &str, secrets: &SecretValues) -> String {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for secret in &secrets.values {
        push_spans(url, secret, &mut spans);
    }
    for (start, end) in userinfo_regions(url) {
        let userinfo = &url[start..end];
        // A `user:pass` userinfo keeps its username and masks the password; a username-only
        // userinfo is masked whole because it is the credential itself.
        let password_start = match userinfo.find(':') {
            Some(colon) if !is_opaque_scheme(&userinfo[..colon]) => start + colon + 1,
            Some(_) => start,
            None => start,
        };
        if password_start < end {
            spans.push((password_start, end));
        }
    }
    if spans.is_empty() {
        return url.to_owned();
    }
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    let mut span = spans[0];
    for &(start, end) in &spans[1..] {
        if start < span.1 {
            span.1 = span.1.max(end);
        } else {
            merged.push(span);
            span = (start, end);
        }
    }
    merged.push(span);
    let mut masked = String::with_capacity(url.len() + MASK.len() * merged.len());
    let mut cursor = 0;
    for (start, end) in merged {
        masked.push_str(&url[cursor..start]);
        masked.push_str(MASK);
        cursor = end;
    }
    masked.push_str(&url[cursor..]);
    masked
}

/// Every `(userinfo_start, userinfo_end)` byte region of a URL, in order and non-overlapping:
/// the top-level authority's, one after each `://`, and one for a schemeless `user:pass@host`
/// shape found at a component boundary. The last group is what makes the userinfo contract
/// hold for a URL nested in another URL's query, fragment or path, and for a second
/// schemeless authority later in the string. A region is only emitted when the text really
/// looks like a userinfo: non-empty, and either it carries a `:` or it has no whitespace (a
/// PAT-like username), and its first word is not an opaque scheme such as `mailto`.
fn userinfo_regions(url: &str) -> Vec<(usize, usize)> {
    let mut regions = Vec::new();
    for start in authority_starts(url) {
        let end = url[start..]
            .find(['/', '?', '#'])
            .map(|index| start + index)
            .unwrap_or(url.len());
        if let Some(at) = url[start..end].rfind('@') {
            regions.push((start, start + at));
        }
    }
    let mut from = 0;
    while let Some(offset) = url[from..].find('@') {
        let at = from + offset;
        let start = url[..at]
            .rfind(['/', '?', '#', '&', '='])
            .map(|index| index + 1)
            .unwrap_or(0);
        if start < at {
            regions.push((start, at));
        }
        from = at + 1;
    }
    regions.retain(|(start, end)| {
        let userinfo = &url[*start..*end];
        if userinfo.is_empty() {
            return false;
        }
        if userinfo.chars().any(char::is_whitespace) && !userinfo.contains(':') {
            return false;
        }
        match userinfo.find(':') {
            Some(colon) => !is_opaque_scheme(&userinfo[..colon]),
            None => true,
        }
    });
    regions.sort_unstable();
    regions.dedup();
    // Overlapping regions are MERGED, never dropped: a userinfo may hold more than one `@`
    // (`p@ssw0rd` as a password, `user@tenant:pass@server`), and the `@`-scan emits a region
    // per `@`. Dropping the longer one would stop the mask at the first `@` and leave the
    // password tail in the clear. Userinfo runs to the LAST `@`, which is what merging gives.
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(regions.len());
    for region in regions {
        match merged.last_mut() {
            Some(last) if region.0 <= last.1 => last.1 = last.1.max(region.1),
            _ => merged.push(region),
        }
    }
    merged
}

/// The headers whose value is a credential whatever it contains. Matched on the name's
/// visible ASCII only: a header name is an ASCII token (RFC 9110 §5.1), so whitespace and
/// anything outside ASCII — including invisible format characters such as U+200B, U+2060,
/// the BOM and the soft hyphen, which `trim()` keeps — is padding, and a padded name is
/// still that header.
fn is_credential_header(name: &str) -> bool {
    let visible: String = name.chars().filter(char::is_ascii_graphic).collect();
    CREDENTIAL_HEADERS
        .iter()
        .any(|credential| visible.eq_ignore_ascii_case(credential))
}

fn is_maskable(value: &str) -> bool {
    value.chars().count() >= MIN_SECRET_CHARS && !value.contains(MASK_CHAR)
}

/// Where an authority can begin, wherever it occurs: the top-level URL's own authority, and
/// one after every `://` (which covers `scheme://`, a malformed `1://` / `my_app://`, and a
/// nested URL's own scheme). Sorted and deduplicated.
fn authority_starts(url: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    if let Some(top) = authority_start(url) {
        starts.push(top);
    }
    let mut search = 0;
    while let Some(offset) = url[search..].find("://") {
        let marker = search + offset;
        starts.push(marker + 3);
        search = marker + 3;
    }
    starts.sort_unstable();
    starts.dedup();
    starts
}

/// The byte range of a URL's authority (userinfo@host), when it has one.
fn authority_bounds(url: &str) -> Option<(usize, usize)> {
    let start = authority_start(url)?;
    let end = url[start..]
        .find(['/', '?', '#'])
        .map(|index| start + index)
        .unwrap_or(url.len());
    Some((start, end))
}

/// Where a URL's authority begins: after `scheme://`, after a leading `//`, after any `://`
/// that comes before the first `@`, or at byte 0 for the schemeless `user:pass@host/…`
/// shape. `None` when the URL carries no authority — an opaque `mailto:`-style path, or text
/// whose part before the `@` is not a URL userinfo (it is empty, or holds whitespace and no
/// `:`, like prose around an email address).
fn authority_start(url: &str) -> Option<usize> {
    if url.starts_with("//") {
        return Some(2);
    }
    if let Some(scheme_end) = scheme_prefix_end(url) {
        return Some(scheme_end + 3);
    }
    // A malformed scheme (`1://`, `my_app://`, a bare `://`) still marks an authority when it
    // precedes the userinfo's `@`; a `://` after the `@` sits in a schemeless URL's path,
    // query or fragment.
    if let Some(marker) = url.find("://")
        && url.find('@').is_none_or(|at| marker < at)
    {
        return Some(marker + 3);
    }
    let first_delimiter = url.find(['/', '?', '#']).unwrap_or(url.len());
    let before_delimiter = &url[..first_delimiter];
    let at = before_delimiter.rfind('@')?;
    let userinfo = &before_delimiter[..at];
    // A literal space is invalid in a userinfo, but a `user:pass` with one is still a
    // credential to mask; without a `:` it is more likely prose around an email address.
    if userinfo.is_empty() || (userinfo.chars().any(char::is_whitespace) && !userinfo.contains(':'))
    {
        return None;
    }
    if let Some(colon) = userinfo.find(':')
        && is_opaque_scheme(&userinfo[..colon])
    {
        return None;
    }
    Some(0)
}

/// Where `scheme://` ends when the URL starts with one (RFC 3986 §3.1: a letter, then
/// letters, digits, `+`, `-` or `.`). A valid scheme prefix marks the authority wherever the
/// `@` is; see [`authority_start`] for a `://` that follows no valid scheme.
fn scheme_prefix_end(url: &str) -> Option<usize> {
    let scheme_end = url.find(':')?;
    let scheme = &url[..scheme_end];
    let mut chars = scheme.chars();
    let starts_with_letter = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic());
    let scheme_chars_only =
        chars.all(|next| next.is_ascii_alphanumeric() || matches!(next, '+' | '-' | '.'));
    (starts_with_letter && scheme_chars_only && url[scheme_end..].starts_with("://"))
        .then_some(scheme_end)
}

fn is_opaque_scheme(word: &str) -> bool {
    OPAQUE_SCHEMES
        .iter()
        .any(|scheme| word.eq_ignore_ascii_case(scheme))
}

/// Mask every URL component whose decoded form hides a secret — path segments, query names,
/// query values and the fragment. Decoded text cannot be mapped back onto the encoded bytes,
/// so a component that hides a secret is masked whole.
fn mask_encoded_parts(url: &str, secrets: &SecretValues) -> String {
    let fragment_start = url.find('#').unwrap_or(url.len());
    let query_start = url[..fragment_start].find('?').unwrap_or(fragment_start);
    let path_from = path_start(url).min(query_start);

    let mut masked = String::with_capacity(url.len());
    masked.push_str(&url[..path_from]);
    push_masked_path(&mut masked, &url[path_from..query_start], secrets);
    if query_start < fragment_start {
        masked.push('?');
        push_masked_query(&mut masked, &url[query_start + 1..fragment_start], secrets);
    }
    if fragment_start < url.len() {
        masked.push('#');
        push_masked_component(&mut masked, &url[fragment_start + 1..], secrets);
    }
    masked
}

/// Where the path begins: after the authority when there is one, else after the scheme's `:`
/// when the first segment names one (`mailto:bob@example.com` is an opaque path, and its
/// scheme is not part of any component), else at the start of the URL.
fn path_start(url: &str) -> usize {
    if let Some((_, end)) = authority_bounds(url) {
        return end;
    }
    let first_delimiter = url.find(['/', '?', '#']).unwrap_or(url.len());
    match url[..first_delimiter].find(':') {
        Some(colon) => colon + 1,
        None => 0,
    }
}

fn push_masked_path(masked: &mut String, path: &str, secrets: &SecretValues) {
    for (index, segment) in path.split('/').enumerate() {
        if index > 0 {
            masked.push('/');
        }
        push_masked_component(masked, segment, secrets);
    }
}

fn push_masked_query(masked: &mut String, query: &str, secrets: &SecretValues) {
    for (index, part) in query.split('&').enumerate() {
        if index > 0 {
            masked.push('&');
        }
        match part.split_once('=') {
            Some((name, value)) => {
                push_masked_component(masked, name, secrets);
                masked.push('=');
                push_masked_component(masked, value, secrets);
            }
            None => push_masked_component(masked, part, secrets),
        }
    }
}

fn push_masked_component(masked: &mut String, part: &str, secrets: &SecretValues) {
    if hides_secret(part, secrets) {
        masked.push_str(MASK);
    } else {
        masked.push_str(part);
    }
}

/// Mask every secret that only the fully decoded URL shows: a secret straddling a raw
/// delimiter with one byte percent-encoded is in no single component's decoded form and not
/// in the raw text. Each occurrence's raw bytes are widened to the delimiters around them, so
/// every component the secret touches is masked whole — decoded text cannot be mapped back
/// to replace only part of a component.
///
/// `+` means a space in a form-encoded query value but itself in a path or fragment
/// (RFC 3986), and a query value need not be form-encoded; both readings are searched, so a
/// secret is caught whichever one the server uses.
fn mask_decoded_spans(url: &str, secrets: &SecretValues) -> String {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for plus_is_space in [false, true] {
        let decoded = decode_with_origins(url, plus_is_space);
        let bytes: Vec<u8> = decoded.iter().map(|&(byte, _, _)| byte).collect();
        for secret in &secrets.values {
            let needle = secret.as_bytes();
            for start in 0..bytes.len() {
                if !bytes
                    .get(start..)
                    .is_some_and(|rest| rest.starts_with(needle))
                {
                    continue;
                }
                let (Some(first), Some(last)) =
                    (decoded.get(start), decoded.get(start + needle.len() - 1))
                else {
                    continue;
                };
                spans.push(widen_to_component(url, first.1, last.2));
            }
        }
    }
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    let mut masked = String::with_capacity(url.len());
    let mut cursor = 0;
    for (start, end) in merged {
        masked.push_str(url.get(cursor..start).unwrap_or_default());
        masked.push_str(MASK);
        cursor = end;
    }
    masked.push_str(url.get(cursor..).unwrap_or_default());
    masked
}

/// The URL percent-decoded byte by byte, each decoded byte paired with the raw byte range it
/// came from. A malformed escape is kept as it is.
fn decode_with_origins(url: &str, plus_is_space: bool) -> Vec<(u8, usize, usize)> {
    let bytes = url.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let escaped = match (byte, bytes.get(index + 1), bytes.get(index + 2)) {
            (b'%', Some(&high), Some(&low)) => hex_digit(high).zip(hex_digit(low)),
            _ => None,
        };
        match escaped {
            Some((high, low)) => {
                decoded.push((high * 16 + low, index, index + 3));
                index += 3;
            }
            None => {
                let byte = if plus_is_space && byte == b'+' {
                    b' '
                } else {
                    byte
                };
                decoded.push((byte, index, index + 1));
                index += 1;
            }
        }
    }
    decoded
}

/// Widen a raw byte range out to the URL delimiters around it, so the components it touches
/// are covered whole; a bound that is somehow not a char boundary widens to that end of the
/// URL, which masks more, never less.
fn widen_to_component(url: &str, start: usize, end: usize) -> (usize, usize) {
    const DELIMITERS: [char; 6] = ['/', '?', '#', '&', '=', '@'];
    let start = url
        .get(..start)
        .and_then(|before| before.rfind(DELIMITERS))
        .map_or(0, |delimiter| delimiter + 1);
    let end = url
        .get(end..)
        .and_then(|after| after.find(DELIMITERS))
        .map_or(url.len(), |delimiter| end + delimiter);
    (start, end)
}

fn hides_secret(value: &str, secrets: &SecretValues) -> bool {
    let decoded = decode_component(value);
    secrets
        .values
        .iter()
        .any(|secret| decoded.contains(secret.as_str()))
}

/// Percent-decode one URL component, treating `+` as the space that form-encoded query
/// strings use. Used for every component when looking for secrets: a raw `+` a secret needs
/// decodes to a space here and so hides nothing from this check, but the literal pass in
/// [`redact_text`] still finds that secret in the clear. A malformed escape is kept as it is:
/// decoding is best effort and never fails.
fn decode_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                match (hex_digit(bytes[index + 1]), hex_digit(bytes[index + 2])) {
                    (Some(high), Some(low)) => {
                        decoded.push(high * 16 + low);
                        index += 3;
                    }
                    _ => {
                        decoded.push(b'%');
                        index += 1;
                    }
                }
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
#[path = "redact_tests.rs"]
mod tests;
