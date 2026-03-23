/// Read-only POST endpoints (path suffixes that are safe for read-only instances)
const READ_ONLY_POST_SUFFIXES: &[&str] = &[
    "_search",
    "_count",
    "_msearch",
    "_mget",
    "_field_caps",
    "_resolve/index",
    "_mapping",
    "_settings",
    "_aliases",
    "_validate/query",
    "_terms_enum",
];

const READ_ONLY_POST_PREFIXES: &[&str] = &["_cat/", "_cluster/"];

/// Check if a request is allowed under ONLY_READ_OPERATIONS rule.
pub fn is_read_allowed(method: &str, path: &str) -> bool {
    let method = method.to_uppercase();

    if method == "GET" {
        return true;
    }

    if method == "POST" {
        // Strip leading slash and query params for matching
        let clean = path.trim_start_matches('/').split('?').next().unwrap_or("");
        // Check suffixes (e.g. /index/_search)
        for suffix in READ_ONLY_POST_SUFFIXES {
            if clean.ends_with(suffix) {
                return true;
            }
        }
        // Check prefixes (e.g. _cat/indices, _cluster/health)
        for prefix in READ_ONLY_POST_PREFIXES {
            if clean.starts_with(prefix) {
                return true;
            }
        }
    }

    false
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
    }

    #[test]
    fn test_post_search_allowed() {
        assert!(is_read_allowed("POST", "/my-index/_search"));
        assert!(is_read_allowed("POST", "/my-index/_search?size=10"));
    }

    #[test]
    fn test_post_count_allowed() {
        assert!(is_read_allowed("POST", "/my-index/_count"));
    }

    #[test]
    fn test_post_msearch_allowed() {
        assert!(is_read_allowed("POST", "/_msearch"));
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
    fn test_post_cluster_allowed() {
        assert!(is_read_allowed("POST", "/_cluster/health"));
        assert!(is_read_allowed("POST", "/_cluster/stats"));
    }

    #[test]
    fn test_post_mapping_allowed() {
        assert!(is_read_allowed("POST", "/my-index/_mapping"));
    }

    #[test]
    fn test_post_validate_query_allowed() {
        assert!(is_read_allowed("POST", "/my-index/_validate/query"));
    }

    #[test]
    fn test_put_blocked() {
        assert!(!is_read_allowed("PUT", "/my-index"));
        assert!(!is_read_allowed("PUT", "/my-index/_mapping"));
    }

    #[test]
    fn test_delete_blocked() {
        assert!(!is_read_allowed("DELETE", "/my-index"));
        assert!(!is_read_allowed("DELETE", "/my-index/_doc/123"));
    }

    #[test]
    fn test_post_index_blocked() {
        assert!(!is_read_allowed("POST", "/my-index/_doc"));
        assert!(!is_read_allowed("POST", "/my-index/_bulk"));
        assert!(!is_read_allowed("POST", "/my-index/_update/123"));
    }

    #[test]
    fn test_post_reindex_blocked() {
        assert!(!is_read_allowed("POST", "/_reindex"));
    }

    #[test]
    fn test_case_insensitive_method() {
        assert!(is_read_allowed("get", "/_cat/indices"));
        assert!(is_read_allowed("post", "/my-index/_search"));
        assert!(!is_read_allowed("put", "/my-index"));
    }
}
