//! Read/write classification for the `ONLY_READ_OPERATIONS` rule.
//!
//! Elasticsearch decides read-vs-write from the (HTTP method, *action*) pair,
//! where the "action" is the reserved path segment that starts with `_`
//! (`_search`, `_doc`, `_bulk`, `_mapping`, ...). Index and alias names can never
//! start with `_`, so the action is unambiguous to find.
//!
//! We classify the SAME way ES routes. Critically we match the action segment
//! **exactly** — never `ends_with` on the whole path — because a document id is
//! attacker-controlled: `POST /idx/_doc/evil_search` ends with `_search` but is a
//! write to the `_doc` action, and a tail match would wave it through.
//!
//! GET/HEAD are *mostly* reads, but not unconditionally: ES accepts several
//! state-changing maintenance actions under GET (e.g. `GET /idx/_refresh`,
//! `GET /idx/_flush`). Those are handled by `WRITE_ACTIONS_ANY_METHOD` and blocked
//! regardless of method. Likewise, a search that opens a **scroll** context
//! (`?scroll=...`) allocates persistent server-side state that this read-only guard
//! can never release (the clear-scroll `DELETE` is itself blocked), so scroll
//! initiation is treated as a write too — even though it rides on `_search`.
//!
//! # Canonicalize before you classify
//!
//! The one invariant that makes this guard sound: **we classify the exact bytes
//! reqwest sends, not our own parse of the raw string.** `server.rs` builds the
//! request URL as `config.url + path` and hands it to reqwest, whose WHATWG `url`
//! crate then normalizes the path *after* any check we could do on the raw string:
//! it converts `\` -> `/` for the http scheme and resolves dot-segments including
//! their percent-encoded spellings (`%2e`, `%2E`, `.%2e`, ...). Classifying the raw
//! string let writes ride in behind a read action — `POST /_search/..\idx/_doc/1`
//! and `POST /_search/%2e%2e/idx/_doc/1` both reach ES as `POST /idx/_doc/1`.
//!
//! So `is_read_allowed` first runs `path` through the *same* `url` crate
//! (`canonical_path`) and classifies the normalized result. This follows the
//! standard guidance for parser-differential / canonicalization bugs: decode and
//! canonicalize to the form that will actually be used, with a single parser, and
//! only then validate. See OWASP Path Traversal and Sonar's "URL parsing
//! differentials" writeup.

/// POST actions that only ever read. Matched against the action segment exactly.
/// Reads of mappings/settings/aliases are done with GET (always allowed), so they
/// are deliberately NOT here — under POST those endpoints mutate state.
const READ_ONLY_POST_ACTIONS: &[&str] = &[
    "_search",       // incl. _search/template
    "_msearch",      // incl. _msearch/template
    "_count",
    "_mget",
    "_field_caps",
    "_validate",     // _validate/query
    "_terms_enum",
    "_resolve",      // _resolve/index, _resolve/cluster
    "_explain",      // _explain/<id> — scoring explanation
    "_analyze",      // tokenization test
    "_search_shards",
    "_rank_eval",
    "_render",       // _render/template
    "_cat",          // _cat/* is read-only in ES
];

/// Actions that mutate cluster/index state even though ES may route them under
/// GET/HEAD. `GET /idx/_refresh` and `GET /idx/_flush` are accepted by ES and change
/// state, so "GET is always a read" is false for these. Blocked for EVERY method
/// (under POST they were already blocked by not being in the read allowlist; listing
/// them here makes the intent explicit and independent of ES version quirks).
const WRITE_ACTIONS_ANY_METHOD: &[&str] = &[
    "_refresh",     // makes buffered writes searchable / opens new segments
    "_flush",       // commits translog to Lucene + fsync (incl. _flush/synced)
    "_forcemerge",  // rewrites segments — heavy I/O
    "_cache",       // _cache/clear — drops caches
];

/// Actions that WRITE (create/update/delete docs, mutate mappings/settings/aliases,
/// open/close/reindex/resize). A legitimate read path never has one of these *behind* a
/// read action: ES anchors routing on the first reserved segment, so `/_search/…/_doc/x`
/// is a no-op on ES 7.10.2. But other ES/OpenSearch versions (and future routes) may
/// parse differently, and "the server will 400 it" is precisely the parser-differential
/// assumption this guard exists to not make. So if a write action appears as any segment
/// after the classified read action, we refuse — a read-action prefix cannot be used as a
/// carrier for a write. Matched exactly against segments, never as a doc-id substring.
const WRITE_ACTIONS: &[&str] = &[
    "_doc",
    "_create",
    "_update",
    "_bulk",
    "_delete_by_query",
    "_update_by_query",
    "_reindex",
    "_mapping",
    "_settings",
    "_aliases",
    "_alias",
    "_open",
    "_close",
    "_split",
    "_shrink",
    "_clone",
    "_rollover",
];

/// `_cluster` is mixed: health/state/stats read, reroute/voting_config_exclusions
/// write. Under POST, allow only these read sub-paths (all also reachable via GET).
const READ_ONLY_CLUSTER_SUBPATHS: &[&str] = &[
    "health",
    "state",
    "stats",
    "pending_tasks",
    "allocation/explain",
    "remote/info",
];

/// Does the path carry a `scroll` query parameter? Initiating a scroll allocates a
/// persistent server-side context that this guard can never release (the clear-scroll
/// `DELETE /_search/scroll` is blocked), so scroll initiation is a write, not a read.
fn has_scroll_param(path: &str) -> bool {
    let query = match path.split_once('?') {
        Some((_, q)) => q,
        None => return false,
    };
    query.split('&').any(|kv| {
        let key = kv.split(['=', '#']).next().unwrap_or("");
        key.eq_ignore_ascii_case("scroll")
    })
}

/// Canonicalize `path` exactly the way the outgoing request will be normalized, so
/// classification sees the same bytes ES receives. Mirrors `server.rs`, which sends
/// `config.url + path`: we ensure a leading slash, join onto a dummy origin, and let
/// reqwest's own `url` crate normalize (backslash -> slash, dot-segment resolution
/// incl. `%2e` forms). Path normalization is host-independent, so the dummy origin is
/// irrelevant to the result. Returns `None` if the path cannot be parsed — the caller
/// treats that as "not a read" (fail closed).
fn canonical_path(path: &str) -> Option<String> {
    let with_slash = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    let url = reqwest::Url::parse(&format!("http://canon.invalid{with_slash}")).ok()?;
    Some(url.path().to_string())
}

/// Check if a request is allowed under the `ONLY_READ_OPERATIONS` rule.
pub fn is_read_allowed(method: &str, path: &str) -> bool {
    let method = method.to_uppercase();

    // PUT/DELETE/PATCH/etc. are always writes. GET/HEAD/POST need action inspection
    // below — GET/HEAD are NOT unconditionally safe (ES accepts some writes via GET).
    if method != "GET" && method != "HEAD" && method != "POST" {
        return false;
    }

    // Canonicalize FIRST (see module docs): classify the normalized path reqwest will
    // actually send, not the raw string. This collapses `\`, `%2e`/`%2E`, `.` and `..`
    // identically to the wire request, closing the guard-vs-ES parse desync at its root
    // rather than blacklisting individual spellings.
    let canon = match canonical_path(path) {
        Some(c) => c,
        None => return false, // unparseable -> fail closed
    };

    // Defense in depth: the `url` crate leaves percent-encoded slash/backslash/dot
    // (`%2f`, `%5c`, a residual `%2e`) ENCODED in the path, but ES may decode them and
    // re-introduce a separator or dot-segment we already resolved past — reopening the
    // desync. Legitimate ES paths never need these encodings, so refuse them outright.
    let lower = canon.to_ascii_lowercase();
    if lower.contains("%2e") || lower.contains("%2f") || lower.contains("%5c") {
        return false;
    }

    // Strip leading slash, drop empty segments. Query is already gone (url.path()).
    let clean = canon.trim_start_matches('/');
    let segments: Vec<&str> = clean.split('/').filter(|s| !s.is_empty()).collect();

    // Canonicalization already resolved dot-segments, but keep this as a backstop in
    // case a future parser leaves one behind. Legitimate ES paths never contain them.
    if segments.iter().any(|s| *s == "." || *s == "..") {
        return false;
    }

    // Find the action: first segment starting with `_`, skipping `_all` (that is
    // the "all indices" selector sitting in an index position, not an action —
    // e.g. `POST /_all/_search`).
    let action_idx = segments
        .iter()
        .position(|s| s.starts_with('_') && *s != "_all");
    let action = action_idx.map(|i| segments[i]);

    // Defense in depth against the "read-action prefix as a carrier" over-permit: the
    // block above classifies on the FIRST reserved segment, but the canonical path may
    // still contain a *write* action further along (`/_search/<junk>/idx/_doc/x`). ES
    // 7.10.2 refuses to route that (write routes don't begin with a read action), yet we
    // must not depend on a specific server's routing. If any segment after the action is
    // itself a write action, the request cannot be a legitimate read — refuse it.
    if let Some(start) = action_idx {
        if segments[start + 1..]
            .iter()
            .any(|s| WRITE_ACTIONS.contains(s))
        {
            return false;
        }
    }

    // State-changing maintenance actions are writes under ANY method — ES accepts
    // several of them via GET, so we must not trust the HTTP method alone.
    if let Some(action) = action {
        if WRITE_ACTIONS_ANY_METHOD.contains(&action) {
            return false;
        }
    }

    // Scroll initiation allocates a context we can't free — treat as a write. Only
    // meaningful on searches; a stray `scroll` param elsewhere is harmless to reject.
    if matches!(action, Some("_search") | Some("_msearch")) && has_scroll_param(path) {
        return false;
    }

    // GET and HEAD are reads for everything else in Elasticsearch.
    if method == "GET" || method == "HEAD" {
        return true;
    }

    // -- POST from here on --

    let idx = match action_idx {
        Some(i) => i,
        None => return false, // no recognized action segment
    };
    let action = segments[idx];

    // `_cluster` is mixed — gate on the specific read sub-path.
    if action == "_cluster" {
        let sub = segments.get(idx + 1..).unwrap_or(&[]).join("/");
        return READ_ONLY_CLUSTER_SUBPATHS.contains(&sub.as_str());
    }

    READ_ONLY_POST_ACTIONS.contains(&action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_always_allowed() {
        assert!(is_read_allowed("GET", "/_cat/indices"));
        assert!(is_read_allowed("GET", "/my-index/_search"));
        assert!(is_read_allowed("GET", "/my-index/_doc/123"));
        assert!(is_read_allowed("GET", "/_cluster/health"));
        // Reads of mapping/settings/aliases go through GET and stay allowed.
        assert!(is_read_allowed("GET", "/my-index/_mapping"));
        assert!(is_read_allowed("GET", "/my-index/_settings"));
        assert!(is_read_allowed("GET", "/_aliases"));
    }

    #[test]
    fn test_head_allowed() {
        // Existence checks are reads.
        assert!(is_read_allowed("HEAD", "/my-index"));
        assert!(is_read_allowed("HEAD", "/my-index/_doc/123"));
    }

    #[test]
    fn test_post_search_allowed() {
        assert!(is_read_allowed("POST", "/my-index/_search"));
        assert!(is_read_allowed("POST", "/my-index/_search?size=10"));
        assert!(is_read_allowed("POST", "/my-index/_search/template"));
        assert!(is_read_allowed("POST", "/_search"));
    }

    #[test]
    fn test_post_count_allowed() {
        assert!(is_read_allowed("POST", "/my-index/_count"));
    }

    #[test]
    fn test_post_msearch_allowed() {
        assert!(is_read_allowed("POST", "/_msearch"));
        assert!(is_read_allowed("POST", "/_msearch/template"));
    }

    #[test]
    fn test_post_mget_allowed() {
        assert!(is_read_allowed("POST", "/_mget"));
        assert!(is_read_allowed("POST", "/my-index/_mget"));
    }

    #[test]
    fn test_post_field_caps_allowed() {
        assert!(is_read_allowed("POST", "/my-index/_field_caps"));
    }

    #[test]
    fn test_post_cat_allowed() {
        assert!(is_read_allowed("POST", "/_cat/indices"));
        assert!(is_read_allowed("POST", "/_cat/shards"));
    }

    #[test]
    fn test_post_validate_query_allowed() {
        assert!(is_read_allowed("POST", "/my-index/_validate/query"));
    }

    #[test]
    fn test_post_extra_reads_allowed() {
        assert!(is_read_allowed("POST", "/my-index/_terms_enum"));
        assert!(is_read_allowed("POST", "/my-index/_explain/123"));
        assert!(is_read_allowed("POST", "/my-index/_analyze"));
        assert!(is_read_allowed("POST", "/_analyze"));
        assert!(is_read_allowed("POST", "/my-index/_search_shards"));
        assert!(is_read_allowed("POST", "/_render/template"));
        assert!(is_read_allowed("POST", "/_resolve/index/foo*"));
    }

    #[test]
    fn test_all_index_selector_read_allowed() {
        // `_all` is an index selector, not an action.
        assert!(is_read_allowed("POST", "/_all/_search"));
        assert!(is_read_allowed("POST", "/_all/_count"));
    }

    // ---- Cluster: mixed read/write ----

    #[test]
    fn test_post_cluster_reads_allowed() {
        assert!(is_read_allowed("POST", "/_cluster/health"));
        assert!(is_read_allowed("POST", "/_cluster/stats"));
        assert!(is_read_allowed("POST", "/_cluster/state"));
        assert!(is_read_allowed("POST", "/_cluster/allocation/explain"));
    }

    #[test]
    fn test_post_cluster_writes_blocked() {
        // These were leaking through the old blanket `_cluster/` prefix.
        assert!(!is_read_allowed("POST", "/_cluster/reroute"));
        assert!(!is_read_allowed("POST", "/_cluster/voting_config_exclusions"));
    }

    // ---- Regression tests for the closed write leaks ----

    #[test]
    fn test_post_mapping_blocked() {
        // POST /_mapping mutates the schema — must be blocked (read via GET).
        assert!(!is_read_allowed("POST", "/my-index/_mapping"));
        assert!(!is_read_allowed("POST", "/_all/_mapping"));
    }

    #[test]
    fn test_post_settings_blocked() {
        assert!(!is_read_allowed("POST", "/my-index/_settings"));
    }

    #[test]
    fn test_post_aliases_blocked() {
        // POST /_aliases mutates cluster state — must be blocked.
        assert!(!is_read_allowed("POST", "/_aliases"));
        assert!(!is_read_allowed("POST", "/my-index/_alias/my-alias"));
    }

    #[test]
    fn test_post_doc_with_readlike_id_blocked() {
        // The `ends_with` bypass: a crafted document id must NOT sneak a write past.
        assert!(!is_read_allowed("POST", "/my-index/_doc/evil_search"));
        assert!(!is_read_allowed("POST", "/my-index/_doc/x_count"));
        assert!(!is_read_allowed("POST", "/my-index/_update/doc_terms_enum"));
    }

    #[test]
    fn test_post_tasks_cancel_blocked() {
        assert!(!is_read_allowed("POST", "/_tasks/nodeid:1/_cancel"));
    }

    #[test]
    fn test_put_blocked() {
        assert!(!is_read_allowed("PUT", "/my-index"));
        assert!(!is_read_allowed("PUT", "/my-index/_mapping"));
        assert!(!is_read_allowed("PUT", "/my-index/_settings"));
        assert!(!is_read_allowed("PUT", "/my-index/_doc/1"));
    }

    #[test]
    fn test_delete_blocked() {
        assert!(!is_read_allowed("DELETE", "/my-index"));
        assert!(!is_read_allowed("DELETE", "/my-index/_doc/123"));
    }

    #[test]
    fn test_post_index_blocked() {
        assert!(!is_read_allowed("POST", "/my-index/_doc"));
        assert!(!is_read_allowed("POST", "/my-index/_doc/1"));
        assert!(!is_read_allowed("POST", "/my-index/_bulk"));
        assert!(!is_read_allowed("POST", "/_bulk"));
        assert!(!is_read_allowed("POST", "/my-index/_update/123"));
        assert!(!is_read_allowed("POST", "/my-index/_update_by_query"));
        assert!(!is_read_allowed("POST", "/my-index/_delete_by_query"));
    }

    #[test]
    fn test_post_reindex_blocked() {
        assert!(!is_read_allowed("POST", "/_reindex"));
    }

    #[test]
    fn test_post_scripts_blocked() {
        assert!(!is_read_allowed("POST", "/_scripts/my-template"));
    }

    // ---- GET-tunneled state-changers: blocked regardless of method ----

    #[test]
    fn test_get_state_changing_actions_blocked() {
        // ES accepts these under GET and they mutate state — must be blocked.
        assert!(!is_read_allowed("GET", "/my-index/_refresh"));
        assert!(!is_read_allowed("GET", "/_refresh"));
        assert!(!is_read_allowed("GET", "/my-index/_flush"));
        assert!(!is_read_allowed("GET", "/my-index/_flush/synced"));
        assert!(!is_read_allowed("GET", "/my-index/_forcemerge?max_num_segments=1"));
        assert!(!is_read_allowed("GET", "/my-index/_cache/clear"));
        // Same actions under POST were already blocked; stay blocked.
        assert!(!is_read_allowed("POST", "/my-index/_refresh"));
        assert!(!is_read_allowed("POST", "/my-index/_forcemerge"));
        // HEAD is not a loophole either.
        assert!(!is_read_allowed("HEAD", "/my-index/_refresh"));
    }

    #[test]
    fn test_refresh_flush_not_confused_with_doc_id() {
        // A doc id that merely contains a write-action word is still a `_doc` write
        // (already blocked), and a read of a doc whose id looks like one is fine.
        assert!(is_read_allowed("GET", "/my-index/_doc/refresh_notes"));
        assert!(!is_read_allowed("POST", "/my-index/_doc/_refresh"));
    }

    // ---- Scroll initiation allocates a context we can't free: treat as write ----

    #[test]
    fn test_scroll_initiation_blocked() {
        assert!(!is_read_allowed("GET", "/my-index/_search?scroll=5m"));
        assert!(!is_read_allowed("POST", "/my-index/_search?scroll=1m&size=100"));
        assert!(!is_read_allowed("GET", "/_search?size=1&scroll=10m"));
        assert!(!is_read_allowed("POST", "/_msearch?scroll=1m"));
    }

    #[test]
    fn test_non_scroll_search_still_allowed() {
        // Ordinary searches (incl. search_after / from-size paging) stay allowed.
        assert!(is_read_allowed("GET", "/my-index/_search"));
        assert!(is_read_allowed("POST", "/my-index/_search?size=100"));
        assert!(is_read_allowed("POST", "/my-index/_search?pretty&size=10"));
        // Scroll continuation without initiating a new context is a read.
        assert!(is_read_allowed("POST", "/_search/scroll"));
        // A `scroll` param on a non-search action is not a search read anyway.
        assert!(is_read_allowed("GET", "/my-index/_doc/123?scroll=5m"));
    }

    // ---- Path-traversal desync: reqwest's url crate resolves `..` before send ----

    #[test]
    fn test_dot_dot_traversal_blocked() {
        // Guard used to see the read action prefix and allow these, but the `url`
        // crate collapses `..` so ES receives a write. Must be blocked.
        assert!(!is_read_allowed("POST", "/_search/../my-index/_doc/1"));
        assert!(!is_read_allowed("POST", "/_msearch/../my-index/_delete_by_query"));
        assert!(!is_read_allowed("POST", "/_count/../_bulk"));
        assert!(!is_read_allowed("POST", "/_cat/../my-index/_doc/3"));
        assert!(!is_read_allowed("GET", "/_search/../my-index/_refresh"));
        // A single dot segment is just as illegitimate.
        assert!(!is_read_allowed("POST", "/./my-index/_doc/1"));
    }

    #[test]
    fn test_legit_paths_without_dot_segments_still_allowed() {
        // The fix must not break normal paths — a doc id that merely contains dots
        // is a single segment, not a dot-segment.
        assert!(is_read_allowed("GET", "/my-index/_doc/1.2.3"));
        assert!(is_read_allowed("GET", "/my-index/_doc/a..b"));
        assert!(is_read_allowed("POST", "/app-logs-2026.04.01/_search"));
    }

    // ---- Encoded / backslash traversal: the `url` crate normalizes AFTER a raw-string
    //      check would run, so classification must happen on the canonicalized path ----

    #[test]
    fn test_backslash_traversal_blocked() {
        // reqwest's url crate converts `\`->`/` for http, so `..\` becomes a real `..`
        // segment on the wire. Guard used to see `..\idx` as one non-dot segment.
        assert!(!is_read_allowed("POST", "/_search/..\\my-index/_doc/1"));
        assert!(!is_read_allowed("POST", "/_count/..\\my-index/_delete_by_query"));
        assert!(!is_read_allowed("POST", "/_msearch/..\\my-index/_bulk"));
        // Backslash + GET-tunneled state-changer.
        assert!(!is_read_allowed("GET", "/_search/..\\my-index/_refresh"));
    }

    #[test]
    fn test_percent_encoded_dot_traversal_blocked() {
        // `%2e`/`%2E` (and mixed `.%2e`) are dot-segments to the url crate.
        assert!(!is_read_allowed("POST", "/_search/%2e%2e/my-index/_doc/1"));
        assert!(!is_read_allowed("POST", "/_search/%2E%2E/my-index/_bulk"));
        assert!(!is_read_allowed("POST", "/_count/.%2e/my-index/_doc/9"));
        assert!(!is_read_allowed("POST", "/_search/%2e%2e/%2e%2e/my-index/_doc/1"));
        assert!(!is_read_allowed("GET", "/_search/%2e%2e/my-index/_refresh"));
    }

    #[test]
    fn test_residual_encoded_separators_blocked() {
        // The url crate leaves `%2f`/`%5c`/a stray `%2e` encoded; ES may decode them.
        // Refuse defensively regardless of the surrounding action.
        assert!(!is_read_allowed("POST", "/my-index%2f_doc%2f1/_search"));
        assert!(!is_read_allowed("GET", "/_cat%2f..%2fmy-index/_doc/1"));
        assert!(!is_read_allowed("POST", "/_search/..%5cmy-index/_doc/1"));
    }

    #[test]
    fn test_encoded_variants_do_not_break_legit_reads() {
        // No encoded dot/slash/backslash in these — must stay allowed.
        assert!(is_read_allowed("GET", "/my-index/_doc/1.2.3"));
        assert!(is_read_allowed("POST", "/app-logs-2026.04.01/_search?size=10"));
        assert!(is_read_allowed("POST", "/_all/_search"));
    }

    // ---- Read-action prefix must not carry a write action in a later segment ----

    #[test]
    fn test_write_action_behind_read_prefix_blocked() {
        // These canonicalize with a read action FIRST but a write action further along.
        // ES 7.10.2 returns "no handler", but the guard must not depend on that.
        assert!(!is_read_allowed("POST", "/_search/junk/my-index/_doc/x"));
        assert!(!is_read_allowed("POST", "/_search/%252e%252e/my-index/_doc/x"));
        assert!(!is_read_allowed("POST", "/_search/..;/my-index/_doc/x"));
        assert!(!is_read_allowed("POST", "/_mget/x/my-index/_bulk"));
        assert!(!is_read_allowed("POST", "/_count/a/b/my-index/_update/1"));
        assert!(!is_read_allowed("GET", "/_search/junk/my-index/_mapping"));
        assert!(!is_read_allowed("POST", "/_cat/a/my-index/_reindex"));
    }

    #[test]
    fn test_read_paths_with_trailing_subresources_still_allowed() {
        // Legit multi-segment reads whose trailing segments are NOT write actions.
        assert!(is_read_allowed("POST", "/my-index/_search/template"));
        assert!(is_read_allowed("POST", "/_render/template"));
        assert!(is_read_allowed("POST", "/_resolve/index/foo*"));
        assert!(is_read_allowed("POST", "/_cluster/allocation/explain"));
        assert!(is_read_allowed("GET", "/_nodes/_local/stats"));
        assert!(is_read_allowed("GET", "/my-index/_doc/123"));
        // A read of the mapping/settings via GET (action == write-word) stays allowed:
        // the write word is the action itself, not a segment *after* it.
        assert!(is_read_allowed("GET", "/my-index/_mapping"));
        assert!(is_read_allowed("GET", "/my-index/_settings"));
    }

    #[test]
    fn test_case_insensitive_method() {
        assert!(is_read_allowed("get", "/_cat/indices"));
        assert!(is_read_allowed("post", "/my-index/_search"));
        assert!(!is_read_allowed("put", "/my-index"));
        assert!(!is_read_allowed("post", "/my-index/_mapping"));
    }
}
