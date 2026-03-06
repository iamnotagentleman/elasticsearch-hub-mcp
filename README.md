# Elasticsech Hub  MCP

MCP server for managing **multiple Elasticsearch instances** with raw Dev Tools-style query execution, per-instance memory, and a shared docs system.

Unlike existing ES MCP servers that only support a single instance with predefined tool shapes, this gives the LLM raw Elasticsearch power with persistent learning across sessions.

## Why this over official MCP?

Elastic’s official [@elastic/mcp-server-elasticsearch](https://github.com/elastic/mcp-server-elasticsearch) exposes four fixed tools (`list_indices`, `get_mappings`, `search`, `get_shards`) and connects to a single cluster. That’s fine for basic exploration, but it limits what you can do.

| | Official MCP | This server |
|---|-------------|-------------|
| **Instances** | One cluster per config | Multiple clusters, each with its own credentials and rules |
| **Query model** | Predefined tools only | Raw Dev Tools–style: any method, path, body |
| **Memory** | None | Per-instance memory — learns mappings, patterns, gotchas across sessions |
| **Safety** | No built-in write protection | `ONLY_READ_OPERATIONS` blocks writes on prod/read-only instances |
| **Flexibility** | Search, mappings, shards | Full API: `_cat/*`, `_cluster/*`, `_count`, `_mget`, `_msearch`, ingest pipelines, etc. |
| **Large results** | Inline only | Results over 10 KB written to temp files to avoid context overflow |

Use the official MCP when you want a simple, opinionated interface to one cluster. Use this when you need multi-cluster access, raw Elasticsearch power, and an LLM that improves over time.

## Features

- **Multiple instances** — configure as many ES clusters as you need, each with its own credentials and access rules
- **Raw query execution** — works exactly like Kibana Dev Tools: any method, any path, any body
- **Query rule enforcement** — mark instances as `ONLY_READ_OPERATIONS` or `ALL_ACCESS`; the server blocks writes on read-only instances
- **Per-instance memory** — the LLM learns about each cluster over time (field mappings, gotchas, useful patterns) and recalls them in future sessions
- **Large result handling** — results over 10 KB are written to temp files to avoid filling the context window
- **Environment variable substitution** — use `${ENV_VAR}` in config for secrets

## Quick Start

### Option 1: Run directly with `uvx` (no clone needed)

If you have [uv](https://docs.astral.sh/uv/) installed, you can run the server directly from GitHub — no cloning required:

```bash
uvx --from git+https://github.com/iamnotagentleman/elasticsearch-hub-mcp elasticsearch-hub-mcp
```

This installs and runs the server in one command. Use this in your MCP client configs too (see [MCP client setup](#add-to-claude-desktop) below).

### Option 2: Install script

No Python or uv needed — one command installs everything:

```bash
curl -LsSf https://raw.githubusercontent.com/iamnotagentleman/elasticsearch-hub-mcp/refs/heads/master/install.sh | sh
```
The script installs uv and Python 3.13 if missing, sets up dependencies, and prints the exact commands to add to your MCP client.

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

**With `uvx` (no clone needed):**

```json
{
  "mcpServers": {
    "elasticsearch": {
      "command": "uvx",
      "args": ["--from", "git+https://github.com/iamnotagentleman/elasticsearch-hub-mcp", "elasticsearch-hub-mcp"],
      "env": {
        "ES_PROD_USER": "elastic",
        "ES_PROD_PASS": "your-password",
        "ES_DEV_KEY": "your-api-key"
      }
    }
  }
}
```

**With local clone:**

```json
{
  "mcpServers": {
    "elasticsearch": {
      "command": "uv",
      "args": ["run", "--directory", "/path/to/elasticsearch-hub-mcp", "elasticsearch-hub-mcp"],
      "env": {
        "ES_PROD_USER": "elastic",
        "ES_PROD_PASS": "your-password",
        "ES_DEV_KEY": "your-api-key"
      }
    }
  }
}
```

Restart Claude Desktop after saving.

### Add to Claude Code

**With `uvx` (no clone needed):**

```bash
claude mcp add elasticsearch -- uvx --from git+https://github.com/iamnotagentleman/elasticsearch-hub-mcp elasticsearch-hub-mcp
```

**With local clone:**

```bash
claude mcp add elasticsearch -- uv run --directory /path/to/elasticsearch-hub-mcp elasticsearch-hub-mcp
```

Or manually add to your `.claude/settings.json` (project-level) or `~/.claude/settings.json` (global):

```json
{
  "mcpServers": {
    "elasticsearch": {
      "command": "uvx",
      "args": ["--from", "git+https://github.com/iamnotagentleman/elasticsearch-hub-mcp", "elasticsearch-hub-mcp"],
      "env": {
        "ES_PROD_USER": "elastic",
        "ES_PROD_PASS": "your-password"
      }
    }
  }
}
```

If your credentials are already in `config.json` (not using `${ENV_VAR}` substitution), you can omit the `env` block.

### Add to Cursor

Open Cursor Settings (`Cmd+,`) > search for **MCP** > click **Add new MCP server**, or manually edit `~/.cursor/mcp.json`:

**With `uvx` (no clone needed):**

```json
{
  "mcpServers": {
    "elasticsearch": {
      "command": "uvx",
      "args": ["--from", "git+https://github.com/iamnotagentleman/elasticsearch-hub-mcp", "elasticsearch-hub-mcp"],
      "env": {
        "ES_PROD_USER": "elastic",
        "ES_PROD_PASS": "your-password"
      }
    }
  }
}
```

**With local clone:**

```json
{
  "mcpServers": {
    "elasticsearch": {
      "command": "uv",
      "args": ["run", "--directory", "/path/to/elasticsearch-hub-mcp", "elasticsearch-hub-mcp"],
      "env": {
        "ES_PROD_USER": "elastic",
        "ES_PROD_PASS": "your-password"
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
| `write_docs` | Write/append to global documentation |
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
- **Storage**: `memories/memory_<instance_name>.json` per instance
- **Persistence**: memories survive server restarts and work across sessions
- **Size protection**: if memories exceed 10 KB, the LLM gets a file path instead of inline content

## Development

```bash
# Install with dev dependencies
uv sync

# Run tests
uv run pytest tests/ -v

# Run the server directly
uv run python -m better_elasticsearch_mcp
```

## Tech stack

- Python 3.13
- [FastMCP 3.x](https://github.com/jlowin/fastmcp) — MCP server framework
- [elasticsearch[async] 8.x](https://elasticsearch-py.readthedocs.io/) — async ES client
- Pydantic v2 — config validation
- uv — package manager
