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

// -- Parameter structs for tools --

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ContentParam {
    #[schemars(description = "The content to write")]
    pub content: String,
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

    #[tool(description = "Overwrite the global documentation. Use for setup-level knowledge that applies across instances.")]
    fn write_docs(&self, Parameters(ContentParam { content }): Parameters<ContentParam>) -> String {
        docs::write_docs(&self.docs_file(), &content)
    }

    #[tool(description = "Append to the global documentation. Use to add new sections without losing existing content.")]
    fn append_docs(&self, Parameters(ContentParam { content }): Parameters<ContentParam>) -> String {
        docs::append_docs(&self.docs_file(), &content)
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

    #[tool(description = "Save a memory about an Elasticsearch instance. Plain text — write whatever is useful.")]
    fn write_memory(
        &self,
        Parameters(WriteMemoryParam {
            instance_name,
            content,
        }): Parameters<WriteMemoryParam>,
    ) -> String {
        memory::write_memory(&self.memories_dir(), &instance_name, &content)
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
            req = req.json(body_val);
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
