# Elasticsearch Hub MCP

MCP server for managing **multiple Elasticsearch instances** with raw Dev Tools-style query execution, per-instance memory, and a shared docs system.

Unlike existing ES MCP servers that only support a single instance with predefined tool shapes, this gives the LLM raw Elasticsearch power with persistent learning across sessions.

## Why this over official MCP?

Elastic's official [@elastic/mcp-server-elasticsearch](https://github.com/elastic/mcp-server-elasticsearch) exposes four fixed tools (`list_indices`, `get_mappings`, `search`, `get_shards`) and connects to a single cluster. That's fine for basic exploration, but it limits what you can do.

| | Official MCP | This server |
|---|-------------|-------------|
| **Instances** | One cluster per config | Multiple clusters, each with its own credentials and rules |
| **Query model** | Predefined tools only | Raw Dev Tools-style: any method, path, body |
| **Memory** | None | Per-instance memory — learns mappings, patterns, gotchas across sessions |
| **Safety** | No built-in write protection | `ONLY_READ_OPERATIONS` blocks writes on prod/read-only instances |
| **Flexibility** | Search, mappings, shards | Full API: `_cat/*`, `_cluster/*`, `_count`, `_mget`, `_msearch`, ingest pipelines, etc. |
| **Large results** | Inline only | Results over 80,000 characters written to temp files to avoid context overflow |

Use the official MCP when you want a simple, opinionated interface to one cluster. Use this when you need multi-cluster access, raw Elasticsearch power, and an LLM that improves over time.

## Features

- **Multiple instances** — configure as many ES clusters as you need, each with its own credentials and access rules
- **Raw query execution** — works exactly like Kibana Dev Tools: any method, any path, any body
- **Query rule enforcement** — mark instances as `ONLY_READ_OPERATIONS` or `ALL_ACCESS`; the server blocks writes on read-only instances
- **Per-instance memory** — the LLM learns about each cluster over time (field mappings, gotchas, useful patterns) and recalls them in future sessions
- **Large result handling** — results over 80,000 characters are written to temp files to avoid filling the context window
- **Environment variable substitution** — use `${ENV_VAR}` in config for secrets

## Quick Start

### Option 1: Install from crates.io

```bash
cargo install elasticsearch-hub-mcp
```

This installs the `elasticsearch-hub-mcp` binary to `~/.cargo/bin/`.

### Option 2: Build from source

```bash
git clone https://github.com/iamnotagentleman/elasticsearch-hub-mcp.git
cd elasticsearch-hub-mcp/rust-server
cargo build --release
```

The binary is at `rust-server/target/release/elasticsearch-hub-mcp`.

### Configure

Copy the example config and fill in your instances:

```bash
cp config.example.json config.json
```

```json
[
  {
    "name": "prod",
    "url": "https://prod-es.example.com:9200",
    "query_rule": "ONLY_READ_OPERATIONS",
    "index_patterns": ["app-logs-*", "metrics-*"],
    "credentials": {
      "type": "basic",
      "username": "${ES_PROD_USER}",
      "password": "${ES_PROD_PASS}"
    },
    "ssl": { "verify_certs": true },
    "default_timeout": 30
  },
  {
    "name": "dev",
    "url": "http://localhost:9200",
    "query_rule": "ALL_ACCESS",
    "index_patterns": ["dev-*"],
    "credentials": { "type": "api_key", "api_key": "${ES_DEV_KEY}" },
    "default_timeout": 15
  }
]
```

Config path is resolved in order: `ES_MCP_CONFIG` env var > `./config.json`.

> **Tip:** Set `ES_MCP_CONFIG` to an absolute path so the server always finds your config regardless of where it runs from:
>
> ```bash
> export ES_MCP_CONFIG=~/.elasticsearch-hub-mcp/config.json
> ```

**Or let Claude generate it for you** — paste this prompt into Claude Code or Claude Desktop:

> I need a config.json for Elasticsearch Hub MCP. I have these clusters:
>
> 1. **name:** prod, **url:** https://prod-es.example.com:9200, **auth:** basic (use ${ES_PROD_USER}/${ES_PROD_PASS}), **query_rule:** ONLY_READ_OPERATIONS, **indices:** app-logs-*, metrics-*, **ssl:** verify_certs true
> 2. **name:** dev, **url:** http://localhost:9200, **auth:** api_key (use ${ES_DEV_KEY}), **query_rule:** ALL_ACCESS, **indices:** dev-*
>
> Write it to config.json

Replace the cluster details with your own. Claude will produce a valid `config.json` and save it.

### Config fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Unique instance identifier (used in all tool calls) |
| `url` | yes | Elasticsearch URL |
| `query_rule` | yes | `ONLY_READ_OPERATIONS` or `ALL_ACCESS` |
| `index_patterns` | yes | Index patterns this instance is known for |
| `credentials` | yes | `basic` (username/password) or `api_key` |
| `ssl` | no | `verify_certs`, `ca_certs` |
| `default_timeout` | no | Request timeout in seconds (default: 30) |

### Add to Claude Desktop

Add to your `claude_desktop_config.json` (macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "elasticsearch": {
      "command": "elasticsearch-hub-mcp",
      "env": {
        "ES_MCP_CONFIG": "/absolute/path/to/config.json"
      }
    }
  }
}
```

> If `~/.cargo/bin` is not in your shell `PATH`, use the full path: `"command": "/Users/you/.cargo/bin/elasticsearch-hub-mcp"`

Restart Claude Desktop after saving.

### Add to Claude Code

```bash
claude mcp add elasticsearch -- elasticsearch-hub-mcp
```

Or manually add to your `.claude/settings.json` (project-level) or `~/.claude/settings.json` (global):

```json
{
  "mcpServers": {
    "elasticsearch": {
      "command": "elasticsearch-hub-mcp",
      "env": {
        "ES_MCP_CONFIG": "/absolute/path/to/config.json"
      }
    }
  }
}
```

### Add to Cursor

Open Cursor Settings (`Cmd+,`) > search for **MCP** > click **Add new MCP server**, or manually edit `~/.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "elasticsearch": {
      "command": "elasticsearch-hub-mcp",
      "env": {
        "ES_MCP_CONFIG": "/absolute/path/to/config.json"
      }
    }
  }
}
```

Restart Cursor after saving. The tools will appear in Cursor's Agent mode.

## Tools

| Tool | Description |
|------|-------------|
| `get_docs` | Read global documentation. **Call first.** |
| `write_docs` | Overwrite global documentation |
| `append_docs` | Append to global documentation |
| `list_instances` | List all instances with name, query rule, and index patterns |
| `get_memory` | Get memory records for an instance (past lessons and context) |
| `write_memory` | Save a lesson or info about an instance |
| `run_query` | Execute a raw ES query (method, path, body) |

### run_query examples

The `run_query` tool works exactly like Kibana Dev Tools:

```
# List indices
run_query("prod", "GET", "/_cat/indices?v&s=index", null)

# Search
run_query("prod", "POST", "/app-logs-*/_search", {"query": {"match": {"message": "error"}}, "size": 10})

# Get mapping
run_query("prod", "GET", "/app-logs-*/_mapping", null)

# Count
run_query("prod", "POST", "/app-logs-*/_count", {"query": {"range": {"@timestamp": {"gte": "now-1h"}}}})

# Cluster health
run_query("prod", "GET", "/_cluster/health", null)

# Aggregation
run_query("prod", "POST", "/app-logs-*/_search", {"size": 0, "aggs": {"status_counts": {"terms": {"field": "status.keyword"}}}})
```

### Query rules

Instances with `ONLY_READ_OPERATIONS` allow:
- All `GET` requests
- `POST` to read endpoints: `_search`, `_count`, `_msearch`, `_mget`, `_field_caps`, `_resolve/index`, `_cat/*`, `_cluster/*`, `_mapping`, `_settings`, `_aliases`, `_validate/query`, `_terms_enum`

Everything else (`PUT`, `DELETE`, write-path `POST`) is blocked server-side.

## Memory system

The LLM automatically builds knowledge about each ES instance over time:

- **What gets saved**: field mappings, date formats, useful query patterns, data relationships, gotchas
- **Storage**: `memories/memory_<instance_name>.md` per instance
- **Persistence**: memories survive server restarts and work across sessions
- **Size protection**: if memories exceed 80,000 characters, the LLM gets a file path instead of inline content

## Development

```bash
cd rust-server

# Build
cargo build

# Run unit tests (26 tests)
cargo test

# Run E2E tests (requires Elasticsearch on localhost:9200)
# See rust-server/test-config.json for the expected config
python3 e2e_test.py

# Build release binary
cargo build --release
```

## Tech stack

- Rust (2024 edition)
- [rmcp 1.2.0](https://github.com/anthropics/rmcp) — MCP server framework
- [reqwest 0.12](https://docs.rs/reqwest) — async HTTP client
- [tokio](https://tokio.rs/) — async runtime
- [serde](https://serde.rs/) / serde_json — serialization
- [schemars](https://graham.cool/schemars/) — JSON Schema generation for tool parameters
