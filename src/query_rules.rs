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

/// Check if a request is allowed under the `ONLY_READ_OPERATIONS` rule.
pub fn is_read_allowed(method: &str, path: &str) -> bool {
    let method = method.to_uppercase();

    // GET and HEAD never mutate state in Elasticsearch.
    if method == "GET" || method == "HEAD" {
        return true;
    }

    // Only POST can be a read; PUT/DELETE/PATCH/etc. are always writes here.
    if method != "POST" {
        return false;
    }

    // Normalize: strip leading slash + query string, drop empty segments.
    let clean = path.trim_start_matches('/').split('?').next().unwrap_or("");
    let segments: Vec<&str> = clean.split('/').filter(|s| !s.is_empty()).collect();

    // Find the action: first segment starting with `_`, skipping `_all` (that is
    // the "all indices" selector sitting in an index position, not an action —
    // e.g. `POST /_all/_search`).
    let action_idx = segments
        .iter()
        .position(|s| s.starts_with('_') && *s != "_all");
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

    #[test]
    fn test_case_insensitive_method() {
        assert!(is_read_allowed("get", "/_cat/indices"));
        assert!(is_read_allowed("post", "/my-index/_search"));
        assert!(!is_read_allowed("put", "/my-index"));
        assert!(!is_read_allowed("post", "/my-index/_mapping"));
    }
}
