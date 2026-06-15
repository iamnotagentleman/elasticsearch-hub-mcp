use crate::config::{ElasticsearchInstance, QueryRule};
use crate::connection_manager::ConnectionManager;
use crate::docs;
use crate::memory;
use crate::query_rules::is_read_allowed;
use chrono::Utc;
use reqwest::Method;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, schemars, tool, tool_handler, tool_router};
use serde_json::Value;
use std::path::PathBuf;
use uuid::Uuid;

const RESULT_SIZE_LIMIT: usize = 80_000;

const SYSTEM_INSTRUCTIONS: &str = r#"You are connected to the Better Elasticsearch MCP server, which manages multiple Elasticsearch instances.

## Startup Sequence (ALWAYS follow this order)
1. Call `get_docs()` first — read general documentation about this setup
2. Call `list_instances()` — see available ES instances, their query rules, and index patterns
3. Call `get_memory(instance_name)` for the relevant instance — read past lessons and context

## First-Time Setup
- If docs are empty and memories are missing, call `init()` to run guided discovery across all instances.
- `init()` backs up existing docs/memories automatically, then returns a discovery framework for you to follow.
- You can call `init()` anytime to re-discover and refresh the knowledge base.

## Core Principles

### Memory System
- Before querying an instance, ALWAYS call `get_memory()` for it. Past lessons save time and prevent repeated mistakes.
- After discovering something important (field mappings, gotchas, useful queries, data patterns), call `write_memory()` with type "info" or "lessons_learned".
- Memory is persistent across sessions. What you learn today helps tomorrow.
- Only write genuinely useful memories — not every query result.

### Running Queries
- `run_query` works exactly like Elasticsearch Dev Tools / Kibana console.
- Format: method (GET/POST/PUT/DELETE), path, and optional body.
- Examples:
  - List indices: `run_query(instance, "GET", "/_cat/indices?v&s=index", null)`
  - Search: `run_query(instance, "POST", "/my-index/_search", {"query": {"match": {"field": "value"}}, "size": 10})`
  - Get mapping: `run_query(instance, "GET", "/my-index/_mapping", null)`
  - Count: `run_query(instance, "POST", "/my-index/_count", {"query": {"range": {"@timestamp": {"gte": "now-1h"}}}})`
  - Get document: `run_query(instance, "GET", "/my-index/_doc/doc-id-123", null)`
  - Cluster health: `run_query(instance, "GET", "/_cluster/health", null)`
  - Aggregate: `run_query(instance, "POST", "/my-index/_search", {"size": 0, "aggs": {"status_counts": {"terms": {"field": "status.keyword"}}}})`

### Query Rules
- Instances with `ONLY_READ_OPERATIONS` only accept read queries. Don't attempt writes.
- Instances with `ALL_ACCESS` allow everything — but still be careful with writes.

### Large Results
- If a result exceeds 80,000 characters, it's saved to a temp file. The response tells you the file path.
- You can read the file directly, use Python, or any other method to extract the relevant data.
- Always use `size` parameter in searches to limit results. Start small (10-20), increase if needed.

### Index Patterns
- Each instance lists its `index_patterns` — these tell you which indices are relevant.
- Use these patterns to pick the right instance for a query.
- When unsure which index to use, check `/_cat/indices?v&s=index` first.

### Best Practices
- Start with small queries and refine. Don't pull large datasets upfront.
- Use `_count` before `_search` to understand data volume.
- Check `_mapping` before writing complex queries — know the field types.
- Use `source_includes` in searches to only fetch fields you need: `{"_source": ["field1", "field2"]}`.
- For time-based data, always filter by time range to avoid scanning too much data.
- Write memories for: field name conventions, date formats, useful query patterns, data relationships between indices, gotchas.

### Documentation
- If you discover something that applies globally (not instance-specific), use `write_docs()` to save it.
- Docs are for setup-level knowledge: which instance to use for what, cross-instance relationships, general tips.
"#;

const INIT_PROMPT_ALL: &str = r#"Discover and document all configured Elasticsearch instances. Follow these phases in order.

## Phase 1: Survey the landscape

1. Call `list_instances()` to see all configured instances.
2. Call `get_docs()` to read existing global documentation.
3. For each instance, call `get_memory(instance_name)` to check what's already known.

Note which instances need discovery (empty or thin memories) vs ones already well-documented.

## Phase 2: Parallel discovery — launch one sub-agent per instance

For EACH instance, launch a separate sub-agent. Each agent explores one instance independently. This is faster (parallel I/O) and keeps each agent's context clean and focused.

SUB-AGENT TASK TEMPLATE (fill in <INSTANCE_NAME> and <ENV> for each):
"#;

const INIT_PROMPT_SINGLE: &str = r#"Discover and document a single Elasticsearch instance. Follow these phases in order.

## Phase 1: Survey

1. Call `get_docs()` to read existing global documentation.
2. Call `get_memory(instance_name)` to check what's already known for this instance.

## Phase 2: Discovery

Explore the target instance directly (no sub-agent needed for a single instance).

DISCOVERY TASK:
"#;

const INIT_PROMPT_ASK: &str = r#"The user called `init()` without specifying an instance. Ask them what they'd like to initialize.

Present the available instances listed below and ask:
- "Should I discover and document **all instances**, or a specific one?"
- List each instance with its name and environment so they can pick.
- If they pick one, proceed with single-instance discovery.
- If they pick all, proceed with parallel discovery across everything.

**Available instances:**
"#;

const INIT_DISCOVERY_TASK: &str = r#"
> You are exploring Elasticsearch instance `<INSTANCE_NAME>` (environment: `<ENV>`).
> Your job: understand what this instance is, what data it holds, and document it.
>
> Run these queries in order using `run_query`:
>
> 1. `GET /_cluster/health` — cluster status, node count, shard health
> 2. `GET /_cat/indices?v&s=docs.count:desc&format=json` — all indices sorted by size
> 3. `GET /_cat/aliases?v&format=json` — alias-to-index mappings
>
> **Classify the instance** based on index naming patterns:
>
> | Signal | Classification |
> |--------|---------------|
> | Time-based index names (`app-logs-2026.04.01`, `*-2026.*`, daily/monthly rollover) | **Log aggregator** — services ship logs here |
> | Stable named indices (`users`, `products`, `orders`) | **Application database** — persistent data store |
> | `metricbeat-*`, `apm-*`, `metrics-*` | **Metrics/APM store** |
> | `*-search-*`, `*-autocomplete`, heavy `text` field mappings | **Search engine** — read-heavy, denormalized |
> | Mix of the above | **Hybrid** — document each usage separately |
>
> **Deep dive into key indices.** For each distinct index pattern group, pick the most representative index (latest time-based or largest) and run:
>
> - `GET /<index>/_mapping` — field structure
> - `POST /<index>/_search` with `{"size": 1}` — see a real document
>
> Focus on what saves time in future queries:
> - Which fields are `.keyword` suffixed (aggregatable)?
> - What date format does `@timestamp` or equivalent use?
> - Are there enum-like fields (`level`, `status`, `type`)? What values do they have?
> - Field naming convention: camelCase, snake_case, dot.notation?
> - Any misleading field names? (e.g., `message` containing structured JSON, `level` with inconsistent casing like "Error" vs "ERROR")
> - Nested objects or flattened fields?
>
> **Write memory.** Call `write_memory(instance_name, content)` with a concise, actionable summary:
> - Instance classification (log aggregator / app DB / metrics / search / hybrid)
> - Key indices found and their purpose
> - Field mapping summary for the most important indices — field names, types, gotchas
> - Data patterns: date formats, casing inconsistencies, nested structures
> - Useful query patterns specific to this data shape
>
> **Return a short summary** (3-5 lines) back to the parent: classification, key indices, and most important finding.
"#;

const INIT_PROMPT_DOCS_PHASE: &str = r#"
## Write global docs

After discovery completes, synthesize `docs.md` by calling `write_docs(content)` with:

- **Instance map** — table of instances: name, environment, classification, what data it holds
- **Environment pairs** — which instances mirror each other (QA ↔ PROD)
- **Cross-instance relationships** — same services logging to different environments, shared index patterns
- **When to use which instance** — clear guidance for common query scenarios
- **Behavioral rules:**
  - Always ask which environment before running queries if not specified
  - After completing queries, check if anything is worth saving to memory
  - Only write genuinely useful memories, not every query result
"#;

const INIT_PROMPT_SUMMARY_PHASE: &str = r#"
## Summary

Report to the user:
- How many instances were explored
- Classification of each instance (one line each)
- Key findings — anything surprising or particularly useful
- What was written to docs and memories
- Suggestions for the user (e.g., "checkout-elk has inconsistent log levels — consider standardizing")
"#;

// -- Parameter structs for tools --

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub enum WriteMode {
    #[default]
    #[serde(rename = "append")]
    Append,
    #[serde(rename = "overwrite")]
    Overwrite,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WriteDocsParam {
    #[schemars(description = "The content to write")]
    pub content: String,
    #[schemars(description = "Write mode: 'append' adds to existing content (default), 'overwrite' replaces everything")]
    #[serde(default)]
    pub write_mode: WriteMode,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InstanceNameParam {
    #[schemars(description = "The Elasticsearch instance name")]
    pub instance_name: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WriteMemoryParam {
    #[schemars(description = "The ES instance this memory is about")]
    pub instance_name: String,
    #[schemars(description = "The memory content — what you learned or discovered")]
    pub content: String,
    #[schemars(description = "Write mode: 'append' adds to existing memories (default), 'overwrite' replaces all memories for this instance")]
    #[serde(default)]
    pub write_mode: WriteMode,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InitParam {
    #[schemars(description = "Optional: specific instance to discover. If omitted, the LLM will ask the user whether to init all or pick one.")]
    pub instance_name: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RunQueryParam {
    #[schemars(description = "Which ES instance to query")]
    pub instance_name: String,
    #[schemars(description = "HTTP method (GET, POST, PUT, DELETE)")]
    pub method: String,
    #[schemars(description = "ES API path (e.g. /_cat/indices?v, /my-index/_search)")]
    pub path: String,
    #[schemars(description = "Optional JSON body (query DSL, mapping, etc.)")]
    pub body: Option<Value>,
    #[schemars(description = "Optional human-readable explanation of what this query does")]
    pub description: Option<String>,
}

// -- Server struct --

#[derive(Debug, Clone)]
pub struct ElasticsearchMcpServer {
    tool_router: ToolRouter<Self>,
    connection_manager: std::sync::Arc<ConnectionManager>,
    project_root: PathBuf,
}

impl ElasticsearchMcpServer {
    pub fn new(
        connection_manager: ConnectionManager,
        project_root: PathBuf,
    ) -> Self {
        Self {
            tool_router: Self::tool_router(),
            connection_manager: std::sync::Arc::new(connection_manager),
            project_root,
        }
    }

    fn memories_dir(&self) -> PathBuf {
        self.project_root.join("memories")
    }

    fn docs_file(&self) -> PathBuf {
        self.project_root.join("docs.md")
    }

    fn tmp_dir(&self) -> PathBuf {
        self.project_root.join(".tmp")
    }

    fn truncate_result(&self, instance_name: &str, result: &str) -> String {
        if result.len() < RESULT_SIZE_LIMIT {
            return result.to_string();
        }

        let tmp_dir = self.tmp_dir();
        std::fs::create_dir_all(&tmp_dir).ok();

        let ts = Utc::now().format("%Y%m%d_%H%M%S");
        let uid = &Uuid::new_v4().to_string()[..6];
        let filename = format!("{}_{ts}_{uid}.json", instance_name);
        let filepath = tmp_dir.join(&filename);

        if let Err(e) = std::fs::write(&filepath, result) {
            return format!("Error writing temp file: {}", e);
        }

        let char_count = result.len();
        format!(
            "Result ({} characters) exceeds maximum allowed {} characters. \
             Full output saved at: {}\n\
             You can read the file directly or use any method you prefer to extract the relevant data.",
            char_count, RESULT_SIZE_LIMIT, filepath.display()
        )
    }

    fn backups_dir(&self) -> PathBuf {
        self.project_root.join("backups")
    }

    /// Back up existing docs.md and memory files before init overwrites them.
    /// If `instance_name` is Some, only back up that instance's memory. Otherwise back up all.
    fn backup_existing_files(&self, instance_name: Option<&str>) -> String {
        let ts = Utc::now().format("%Y%m%d_%H%M%S");
        let backup_dir = self.backups_dir();
        std::fs::create_dir_all(&backup_dir).ok();

        let mut backed_up = Vec::new();

        // Backup docs.md (always — docs are global)
        let docs = self.docs_file();
        if docs.exists() {
            if let Ok(content) = std::fs::read_to_string(&docs) {
                if !content.trim().is_empty() {
                    let dest = backup_dir.join(format!("docs_{}.md", ts));
                    if std::fs::copy(&docs, &dest).is_ok() {
                        backed_up.push(format!(
                            "- docs.md -> backups/{}",
                            dest.file_name().unwrap_or_default().to_string_lossy()
                        ));
                    }
                }
            }
        }

        // Backup memory files — scoped to instance if specified
        let memories_dir = self.memories_dir();
        if memories_dir.exists() {
            if let Some(name) = instance_name {
                // Single instance: only back up its memory file(s)
                for ext in &["md", "json"] {
                    let path = memories_dir.join(format!("memory_{}.{}", name, ext));
                    if path.exists() {
                        let dest = backup_dir.join(format!("memory_{}_{}.{}", name, ts, ext));
                        if std::fs::copy(&path, &dest).is_ok() {
                            backed_up.push(format!(
                                "- memory_{}.{} -> backups/{}",
                                name,
                                ext,
                                dest.file_name().unwrap_or_default().to_string_lossy()
                            ));
                        }
                    }
                }
            } else {
                // All instances: back up every memory file
                if let Ok(entries) = std::fs::read_dir(&memories_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                        if ext == "md" || ext == "json" {
                            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                let dest = backup_dir.join(format!("{}_{}.{}", stem, ts, ext));
                                if std::fs::copy(&path, &dest).is_ok() {
                                    backed_up.push(format!(
                                        "- {} -> backups/{}",
                                        path.file_name().unwrap_or_default().to_string_lossy(),
                                        dest.file_name().unwrap_or_default().to_string_lossy()
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        if backed_up.is_empty() {
            "No existing docs or memories found — clean slate.\n".to_string()
        } else {
            format!(
                "Backed up {} file(s) to backups/ (timestamped). You can safely overwrite docs and memories — originals are preserved.\n{}\n",
                backed_up.len(),
                backed_up.join("\n")
            )
        }
    }

    fn format_instances(instances: Vec<&ElasticsearchInstance>) -> String {
        let result: Vec<Value> = instances
            .iter()
            .map(|inst| {
                serde_json::json!({
                    "name": inst.name,
                    "url": inst.url,
                    "environment": inst.environment,
                    "query_rule": inst.query_rule.as_str(),
                    "index_patterns": inst.index_patterns,
                    "default_timeout": inst.default_timeout,
                })
            })
            .collect();
        serde_json::to_string_pretty(&result).unwrap_or_else(|_| "[]".to_string())
    }
}

#[tool_router]
impl ElasticsearchMcpServer {
    #[tool(description = "Get general documentation about this Elasticsearch setup. Call this FIRST before anything else.")]
    fn get_docs(&self) -> String {
        docs::get_docs(&self.docs_file())
    }

    #[tool(description = "Write global documentation. Use for setup-level knowledge that applies across instances. Defaults to append; use write_mode='overwrite' to replace everything.")]
    fn write_docs(&self, Parameters(WriteDocsParam { content, write_mode }): Parameters<WriteDocsParam>) -> String {
        match write_mode {
            WriteMode::Append => docs::append_docs(&self.docs_file(), &content),
            WriteMode::Overwrite => docs::write_docs(&self.docs_file(), &content),
        }
    }

    #[tool(description = "List all configured Elasticsearch instances with their query rules and index patterns. Call after get_docs().")]
    fn list_instances(&self) -> String {
        let instances = self.connection_manager.list_instances();
        Self::format_instances(instances)
    }

    #[tool(description = "Get memory records for an Elasticsearch instance. Call this before querying an instance to learn from past sessions.")]
    fn get_memory(
        &self,
        Parameters(InstanceNameParam { instance_name }): Parameters<InstanceNameParam>,
    ) -> String {
        memory::get_memories(&self.memories_dir(), &instance_name)
    }

    #[tool(description = "Save a memory about an Elasticsearch instance. Plain text — write whatever is useful. Defaults to append; use write_mode='overwrite' to replace all memories for this instance.")]
    fn write_memory(
        &self,
        Parameters(WriteMemoryParam {
            instance_name,
            content,
            write_mode,
        }): Parameters<WriteMemoryParam>,
    ) -> String {
        match write_mode {
            WriteMode::Append => memory::write_memory(&self.memories_dir(), &instance_name, &content),
            WriteMode::Overwrite => memory::overwrite_memory(&self.memories_dir(), &instance_name, &content),
        }
    }

    #[tool(description = "Discover and document Elasticsearch instances. Backs up existing docs/memories, then returns a guided discovery framework. Pass instance_name to init a specific instance, or omit to choose.")]
    fn init(
        &self,
        Parameters(InitParam { instance_name }): Parameters<InitParam>,
    ) -> String {
        let instances = self.connection_manager.list_instances();

        // Validate instance_name if provided
        if let Some(ref name) = instance_name {
            if self.connection_manager.get_instance_config(name).is_err() {
                let available: Vec<String> = instances
                    .iter()
                    .map(|i| format!("- {} ({})", i.name, i.environment))
                    .collect();
                return format!(
                    "Unknown instance '{}'. Available instances:\n{}",
                    name,
                    available.join("\n")
                );
            }
        }

        let backup_report = self.backup_existing_files(instance_name.as_deref());

        let prompt = match instance_name {
            Some(ref name) => {
                let config = self.connection_manager.get_instance_config(name).unwrap();
                let filled_task = INIT_DISCOVERY_TASK
                    .replace("<INSTANCE_NAME>", &config.name)
                    .replace("<ENV>", &config.environment);
                format!(
                    "{}\n{}{}\n{}\n{}",
                    backup_report,
                    INIT_PROMPT_SINGLE,
                    filled_task,
                    INIT_PROMPT_DOCS_PHASE,
                    INIT_PROMPT_SUMMARY_PHASE
                )
            }
            None => {
                let instance_list: Vec<String> = instances
                    .iter()
                    .map(|i| format!("- **{}** (environment: {}, query_rule: {})", i.name, i.environment, i.query_rule.as_str()))
                    .collect();
                format!(
                    "{}\n{}\n{}\n{}\n{}\n{}\n{}",
                    backup_report,
                    INIT_PROMPT_ASK,
                    instance_list.join("\n"),
                    INIT_PROMPT_ALL,
                    INIT_DISCOVERY_TASK,
                    INIT_PROMPT_DOCS_PHASE,
                    INIT_PROMPT_SUMMARY_PHASE
                )
            }
        };

        prompt
    }

    #[tool(description = "Execute a raw Elasticsearch query, like Kibana Dev Tools console.")]
    async fn run_query(
        &self,
        Parameters(RunQueryParam {
            instance_name,
            method,
            path,
            body,
            description: _description,
        }): Parameters<RunQueryParam>,
    ) -> String {
        // Validate instance exists
        let config = match self.connection_manager.get_instance_config(&instance_name) {
            Ok(c) => c.clone(),
            Err(e) => return e,
        };
        let client = match self.connection_manager.get_client(&instance_name) {
            Ok(c) => c,
            Err(e) => return e,
        };

        // Enforce query rules
        if config.query_rule == QueryRule::OnlyReadOperations && !is_read_allowed(&method, &path) {
            return format!(
                "Instance '{}' is read-only. Only read operations are allowed.",
                instance_name
            );
        }

        // Ensure path starts with /
        let path = if path.starts_with('/') {
            path
        } else {
            format!("/{}", path)
        };

        let url = format!("{}{}", config.url, path);
        let http_method = match method.to_uppercase().as_str() {
            "GET" => Method::GET,
            "POST" => Method::POST,
            "PUT" => Method::PUT,
            "DELETE" => Method::DELETE,
            "HEAD" => Method::HEAD,
            other => return format!("Unsupported HTTP method: {}", other),
        };

        let mut req = client.request(http_method, &url);
        if let Some(ref body_val) = body {
            // MCP clients frequently send the body as a JSON-encoded string
            // (e.g. "{\"query\":...}"). Sending that through .json() would
            // double-encode it and ES rejects it with
            // "Expected [START_OBJECT] but found [VALUE_STRING]".
            let body_val = match body_val {
                Value::String(s) => {
                    serde_json::from_str::<Value>(s).unwrap_or_else(|_| body_val.clone())
                }
                other => other.clone(),
            };
            req = req.json(&body_val);
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let resp_body = match resp.text().await {
                    Ok(t) => t,
                    Err(e) => return format!("Error reading response from '{}': {}", instance_name, e),
                };

                // Try to parse as JSON for pretty output
                match serde_json::from_str::<Value>(&resp_body) {
                    Ok(parsed) => {
                        if status >= 400 {
                            let reason = if let Some(error) = parsed.get("error") {
                                if let Some(reason) = error.get("reason").and_then(|r| r.as_str()) {
                                    reason.to_string()
                                } else {
                                    error.to_string()
                                }
                            } else {
                                resp_body[..resp_body.len().min(500)].to_string()
                            };
                            return format!(
                                "Elasticsearch error on '{}': {} - {}",
                                instance_name, status, reason
                            );
                        }
                        let result = serde_json::to_string_pretty(&parsed)
                            .unwrap_or(resp_body);
                        self.truncate_result(&instance_name, &result)
                    }
                    Err(_) => {
                        if status >= 400 {
                            return format!(
                                "Elasticsearch error on '{}': {} - {}",
                                instance_name,
                                status,
                                &resp_body[..resp_body.len().min(500)]
                            );
                        }
                        self.truncate_result(&instance_name, &resp_body)
                    }
                }
            }
            Err(e) => {
                if e.is_timeout() {
                    format!(
                        "Request to '{}' timed out (timeout: {}s). \
                         Try a more specific query or increase the timeout.",
                        instance_name, config.default_timeout
                    )
                } else if e.is_connect() {
                    format!(
                        "Failed to connect to '{}' at {}. Check VPN/network.",
                        instance_name, config.url
                    )
                } else {
                    format!("Request error on '{}': {}", instance_name, e)
                }
            }
        }
    }
}

#[tool_handler]
impl ServerHandler for ElasticsearchMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(SYSTEM_INSTRUCTIONS.to_string())
    }
}
