"""FastMCP server with lifespan management."""

from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from dataclasses import dataclass
from pathlib import Path

from fastmcp import FastMCP

from .config import load_config
from .connection_manager import ConnectionManager

SYSTEM_INSTRUCTIONS = """\
You are connected to the Better Elasticsearch MCP server, which manages multiple Elasticsearch instances.

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
- If a result exceeds 10 KB, it's saved to a temp file. The response tells you the file path.
- Use command line tools (head, grep, jq) on the file to extract what you need without filling context.
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
"""


@dataclass
class AppContext:
    connection_manager: ConnectionManager


@asynccontextmanager
async def app_lifespan(server: FastMCP) -> AsyncIterator[AppContext]:
    """Load config, initialize connections, yield context, cleanup."""
    # Ensure directories exist (use project root, not cwd)
    project_root = Path(__file__).resolve().parent.parent.parent
    (project_root / "memories").mkdir(parents=True, exist_ok=True)
    (project_root / ".tmp").mkdir(parents=True, exist_ok=True)

    instances = load_config()
    cm = ConnectionManager()
    await cm.initialize(instances)

    try:
        yield AppContext(connection_manager=cm)
    finally:
        await cm.close()


mcp = FastMCP(
    "elasticsearch-hub-mcp",
    instructions=SYSTEM_INSTRUCTIONS,
    lifespan=app_lifespan,
)

# Import tools to register them on the mcp instance
from . import tools  # noqa: E402, F401
