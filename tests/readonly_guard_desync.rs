//! Regression: the read-only guard must classify the SAME path reqwest sends.
//!
//! For each crafted path we compute two things:
//!   1. `is_read_allowed(method, path)` — the guard's verdict on the raw string.
//!   2. `reqwest::Url`-normalized path — what ES actually receives on the wire.
//! The invariant: whenever the normalized path resolves to a write/state-changing
//! action, the guard MUST block the raw string. A guard=ALLOW row whose normalized
//! path is a write is exactly the traversal-desync bypass this test guards against.

use elasticsearch_hub_mcp::query_rules::is_read_allowed;

/// Mirror server.rs: ensure leading slash, concat onto origin, read reqwest's path().
fn es_receives(path: &str) -> String {
    let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    match reqwest::Url::parse(&format!("http://host:9200{path}")) {
        Ok(u) => u.path().to_string(),
        Err(e) => format!("<parse error: {e}>"),
    }
}

/// True if the normalized ES path is a write / state-changer (i.e. NOT a read action).
fn normalized_is_write(method: &str, path: &str) -> bool {
    // Reuse the guard against the ALREADY-normalized path: if the guard would block the
    // clean path, that path is a write. (No traversal left in a normalized path, so this
    // reflects the pure action classification.)
    !is_read_allowed(method, &es_receives(path))
}

#[test]
fn smuggled_writes_are_blocked() {
    // (method, raw path the attacker supplies)
    let attacks = [
        ("POST", "/_search/..\\my-index/_doc/1"),        // backslash-smuggled ..
        ("POST", "/_search/%2e%2e/my-index/_doc/1"),     // percent-encoded ..
        ("POST", "/_search/%2E%2E/my-index/_bulk"),      // uppercase pct ..
        ("POST", "/_count/.%2e/my-index/_doc/9"),        // mixed .%2e
        ("POST", "/_count/..\\my-index/_delete_by_query"), // destructive
        ("GET",  "/_search/..\\my-index/_refresh"),      // GET-tunneled state change
        ("POST", "/_search/../my-index/_doc/1"),         // literal control (already fixed)
    ];

    for (method, raw) in attacks {
        let normalized = es_receives(raw);
        // Every one of these normalizes to a write on the wire...
        assert!(
            normalized_is_write(method, raw),
            "test setup wrong: {method} {raw} -> {normalized} is not a write"
        );
        // ...so the guard MUST refuse the raw string.
        assert!(
            !is_read_allowed(method, raw),
            "BYPASS: guard allowed {method} {raw}, but ES receives write {normalized}"
        );
    }
}

#[test]
fn legit_reads_survive_canonicalization() {
    let reads = [
        ("GET",  "/my-index/_doc/1.2.3"),
        ("GET",  "/_cat/indices?v&s=index"),
        ("POST", "/my-index/_search?size=10"),
        ("POST", "/app-logs-2026.04.01/_search"),
        ("POST", "/_all/_count"),
        ("GET",  "/_cluster/health"),
    ];
    for (method, raw) in reads {
        assert!(is_read_allowed(method, raw), "guard wrongly blocked read {method} {raw}");
    }
}
