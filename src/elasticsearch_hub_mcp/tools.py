"""MCP tool definitions for the ES server."""

import json
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

import aiohttp
from fastmcp import Context

from .config import QueryRule
from .docs import get_docs as _get_docs
from .docs import write_docs as _write_docs
from .memory import MemoryObject
from .memory import get_memories as _get_memories
from .memory import write_memory as _write_memory
from .server import AppContext, mcp

RESULT_SIZE_LIMIT = 10 * 1024  # 10 KB
TMP_DIR = Path(__file__).resolve().parent.parent.parent / ".tmp"

# Read-only POST endpoints (path suffixes that are safe for read-only instances)
READ_ONLY_POST_SUFFIXES = (
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
)

READ_ONLY_POST_PREFIXES = (
    "_cat/",
    "_cluster/",
)


def _is_read_allowed(method: str, path: str) -> bool:
    """Check if a request is allowed under ONLY_READ_OPERATIONS rule."""
    method = method.upper()

    if method == "GET":
        return True

    if method == "POST":
        # Strip leading slash and query params for matching
        clean = path.lstrip("/").split("?")[0]
        # Check suffixes (e.g. /index/_search)
        for suffix in READ_ONLY_POST_SUFFIXES:
            if clean.endswith(suffix):
                return True
        # Check prefixes (e.g. _cat/indices, _cluster/health)
        for prefix in READ_ONLY_POST_PREFIXES:
            if clean.startswith(prefix):
                return True

    return False


def _truncate_result(instance_name: str, result: str) -> str:
    """If result exceeds size limit, write to temp file and return notice."""
    if len(result) < RESULT_SIZE_LIMIT:
        return result

    TMP_DIR.mkdir(parents=True, exist_ok=True)
    ts = datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
    uid = uuid.uuid4().hex[:6]
    filename = f"{instance_name}_{ts}_{uid}.json"
    filepath = TMP_DIR / filename
    filepath.write_text(result)

    return (
        f"Result exceeded 10 KB. "
        f"Full output at {filepath}, use command line tools to prevent context fill"
    )


@mcp.tool()
def get_docs() -> str:
    """Get general documentation about this Elasticsearch setup. Call this FIRST before anything else."""
    return _get_docs()


@mcp.tool()
def write_docs(content: str) -> str:
    """Write or append to the global documentation. Use for setup-level knowledge that applies across instances."""
    return _write_docs(content)


@mcp.tool()
def list_instances(ctx: Context) -> str:
    """List all configured Elasticsearch instances with their query rules and index patterns. Call after get_docs()."""
    app: AppContext = ctx.request_context.lifespan_context
    instances = app.connection_manager.list_instances()

    result = []
    for inst in instances:
        result.append(
            {
                "name": inst.name,
                "url": inst.url,
                "environment": inst.environment,
                "query_rule": inst.query_rule.value,
                "index_patterns": inst.index_patterns,
                "default_timeout": inst.default_timeout,
            }
        )

    return json.dumps(result, indent=2)


@mcp.tool()
def get_memory(instance_name: str) -> str:
    """Get memory records for an Elasticsearch instance. Call this before querying an instance to learn from past sessions."""
    return _get_memories(instance_name)


@mcp.tool()
def write_memory(instance_name: str, index: str | None, context: str, type: str) -> str:
    """Save a lesson or info about an Elasticsearch instance.

    Args:
        instance_name: The ES instance this memory is about.
        index: The specific index this memory relates to (null if general).
        context: The actual memory content — what you learned or discovered.
        type: Either "info" (factual) or "lessons_learned" (gotcha/tip).
    """
    memory = MemoryObject(index=index, context=context, type=type)  # type: ignore[arg-type]
    return _write_memory(instance_name, memory)


@mcp.tool()
async def run_query(
    ctx: Context,
    instance_name: str,
    method: str,
    path: str,
    body: dict[str, Any] | None = None,
) -> str:
    """Execute a raw Elasticsearch query, like Kibana Dev Tools console.

    Args:
        instance_name: Which ES instance to query.
        method: HTTP method (GET, POST, PUT, DELETE).
        path: ES API path (e.g. /_cat/indices?v, /my-index/_search).
        body: Optional JSON body (query DSL, mapping, etc.).
    """
    app: AppContext = ctx.request_context.lifespan_context
    cm = app.connection_manager

    # Validate instance exists
    try:
        config = cm.get_instance_config(instance_name)
        session = cm.get_session(instance_name)
    except KeyError as e:
        return str(e)

    # Enforce query rules
    if config.query_rule == QueryRule.ONLY_READ_OPERATIONS:
        if not _is_read_allowed(method, path):
            return (
                f"Instance '{instance_name}' is read-only. "
                f"Only read operations are allowed."
            )

    # Ensure path starts with /
    if not path.startswith("/"):
        path = f"/{path}"

    # Execute the query
    try:
        async with session.request(
            method=method.upper(),
            url=path,
            json=body,
        ) as resp:
            resp_body = await resp.text()

            # Try to parse as JSON for pretty output
            try:
                parsed = json.loads(resp_body)
                # Check for ES error responses
                if resp.status >= 400:
                    error_msg = parsed.get("error", {})
                    if isinstance(error_msg, dict):
                        reason = error_msg.get("reason", resp_body)
                    else:
                        reason = error_msg
                    return f"Elasticsearch error on '{instance_name}': {resp.status} - {reason}"
                result = json.dumps(parsed, indent=2, default=str)
            except json.JSONDecodeError:
                if resp.status >= 400:
                    return f"Elasticsearch error on '{instance_name}': {resp.status} - {resp_body[:500]}"
                result = resp_body

            return _truncate_result(instance_name, result)

    except aiohttp.ServerTimeoutError:
        return (
            f"Request to '{instance_name}' timed out "
            f"(timeout: {config.default_timeout}s). "
            f"Try a more specific query or increase the timeout."
        )
    except aiohttp.ClientConnectorError:
        return (
            f"Failed to connect to '{instance_name}' at {config.url}. "
            f"Check VPN/network."
        )
    except aiohttp.ClientError as e:
        return f"Request error on '{instance_name}': {e}"
