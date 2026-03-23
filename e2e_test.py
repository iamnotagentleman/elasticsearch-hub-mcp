#!/usr/bin/env python3
"""End-to-end tests for the Elasticsearch Hub MCP server (Rust).

Communicates with the server over stdio using MCP JSON-RPC protocol.
"""

import json
import os
import select
import subprocess
import sys
import threading

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
BINARY = os.path.join(SCRIPT_DIR, "target", "debug", "elasticsearch-hub-mcp")
CONFIG = os.path.join(SCRIPT_DIR, "test-config.json")

PASS = 0
FAIL = 0


def run_mcp_session(messages: list[dict], expected_responses: int = 1) -> list[dict]:
    """Send a sequence of MCP messages and collect responses."""
    env = os.environ.copy()
    env["ES_MCP_CONFIG"] = CONFIG
    env["ES_MCP_PROJECT_ROOT"] = SCRIPT_DIR

    proc = subprocess.Popen(
        [BINARY],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
    )

    responses = []

    def read_responses():
        """Read JSON-RPC responses from stdout."""
        buf = b""
        while True:
            chunk = proc.stdout.read(1)
            if not chunk:
                break
            buf += chunk
            if chunk == b"\n":
                line = buf.decode().strip()
                buf = b""
                if line:
                    try:
                        responses.append(json.loads(line))
                    except json.JSONDecodeError:
                        pass
                    if len(responses) >= expected_responses:
                        break

    reader = threading.Thread(target=read_responses, daemon=True)
    reader.start()

    # Send messages one at a time with small delay between them
    for msg in messages:
        line = json.dumps(msg) + "\n"
        proc.stdin.write(line.encode())
        proc.stdin.flush()

    # Wait for responses
    reader.join(timeout=15)

    # Close stdin to let server exit
    try:
        proc.stdin.close()
    except Exception:
        pass
    try:
        proc.kill()
    except Exception:
        pass
    proc.wait()

    return responses


def test(description: str, responses: list[dict], check_fn) -> bool:
    global PASS, FAIL
    try:
        result = check_fn(responses)
        if result:
            print(f"  PASS: {description}")
            PASS += 1
            return True
        else:
            print(f"  FAIL: {description}")
            print(f"    Responses: {json.dumps(responses, indent=2)[:500]}")
            FAIL += 1
            return False
    except Exception as e:
        print(f"  FAIL: {description}")
        print(f"    Error: {e}")
        print(f"    Responses: {json.dumps(responses, indent=2)[:500]}")
        FAIL += 1
        return False


def init_messages():
    return [
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1.0"},
            },
        },
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
    ]


def tool_call(id: int, name: str, arguments: dict):
    return {
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments},
    }


def get_tool_result_text(responses: list[dict], id: int) -> str:
    for resp in responses:
        if resp.get("id") == id and "result" in resp:
            content = resp["result"].get("content", [])
            texts = [c.get("text", "") for c in content if c.get("type") == "text"]
            return " ".join(texts)
    return ""


def main():
    global PASS, FAIL
    print("=== Elasticsearch Hub MCP Server (Rust) - E2E Tests ===\n")

    # Test 1: Server initialization
    print("Test 1: Server initialization")
    msgs = init_messages()
    responses = run_mcp_session(msgs, expected_responses=1)
    test(
        "Initialize handshake returns serverInfo",
        responses,
        lambda r: any("serverInfo" in json.dumps(resp) for resp in r),
    )

    # Test 2: List tools
    print("Test 2: List tools")
    msgs = init_messages() + [
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "List tools includes run_query",
        responses,
        lambda r: any("run_query" in json.dumps(resp) for resp in r),
    )

    # Test 3: get_docs
    print("Test 3: get_docs tool")
    msgs = init_messages() + [tool_call(3, "get_docs", {})]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "get_docs returns content",
        responses,
        lambda r: len(get_tool_result_text(r, 3)) > 0,
    )

    # Test 4: list_instances
    print("Test 4: list_instances tool")
    msgs = init_messages() + [tool_call(4, "list_instances", {})]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "list_instances returns local-test",
        responses,
        lambda r: "local-test" in get_tool_result_text(r, 4),
    )

    # Test 5: run_query - cluster health
    print("Test 5: run_query - cluster health")
    msgs = init_messages() + [
        tool_call(5, "run_query", {
            "instance_name": "local-test",
            "method": "GET",
            "path": "/_cluster/health",
        })
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "Cluster health returns cluster_name",
        responses,
        lambda r: "cluster_name" in get_tool_result_text(r, 5),
    )

    # Test 6: run_query - search
    print("Test 6: run_query - search documents")
    msgs = init_messages() + [
        tool_call(6, "run_query", {
            "instance_name": "local-test",
            "method": "POST",
            "path": "/test-logs-2024/_search",
            "body": {"query": {"match_all": {}}, "size": 10},
        })
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "Search returns hits",
        responses,
        lambda r: "hits" in get_tool_result_text(r, 6),
    )

    # Test 7: run_query - read-only enforcement (DELETE)
    print("Test 7: run_query - read-only enforcement")
    msgs = init_messages() + [
        tool_call(7, "run_query", {
            "instance_name": "local-readonly",
            "method": "DELETE",
            "path": "/test-logs-2024",
        })
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "Read-only blocks DELETE",
        responses,
        lambda r: "read-only" in get_tool_result_text(r, 7),
    )

    # Test 8: run_query - POST _doc blocked on read-only
    print("Test 8: run_query - POST _doc blocked on read-only")
    msgs = init_messages() + [
        tool_call(8, "run_query", {
            "instance_name": "local-readonly",
            "method": "POST",
            "path": "/test-logs-2024/_doc",
            "body": {"message": "should not be indexed"},
        })
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "Read-only blocks POST _doc",
        responses,
        lambda r: "read-only" in get_tool_result_text(r, 8),
    )

    # Test 9: run_query - POST _search allowed on read-only
    print("Test 9: run_query - POST _search allowed on read-only")
    msgs = init_messages() + [
        tool_call(9, "run_query", {
            "instance_name": "local-readonly",
            "method": "POST",
            "path": "/test-logs-2024/_search",
            "body": {"query": {"match_all": {}}, "size": 1},
        })
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "Read-only allows POST _search",
        responses,
        lambda r: "hits" in get_tool_result_text(r, 9),
    )

    # Test 10: write_memory
    print("Test 10: write_memory")
    msgs = init_messages() + [
        tool_call(10, "write_memory", {
            "instance_name": "local-test",
            "content": "test-logs-2024 has @timestamp, message, level fields",
        })
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "write_memory saves successfully",
        responses,
        lambda r: "Memory saved" in get_tool_result_text(r, 10),
    )

    # Test 11: get_memory
    print("Test 11: get_memory")
    msgs = init_messages() + [
        tool_call(11, "get_memory", {"instance_name": "local-test"})
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "get_memory retrieves saved content",
        responses,
        lambda r: "@timestamp" in get_tool_result_text(r, 11),
    )

    # Test 12: write_docs
    print("Test 12: write_docs")
    msgs = init_messages() + [
        tool_call(12, "write_docs", {"content": "## Test Setup\nLocal ES for testing."})
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "write_docs writes successfully",
        responses,
        lambda r: "Documentation written" in get_tool_result_text(r, 12),
    )

    # Test 13: append_docs
    print("Test 13: append_docs")
    msgs = init_messages() + [
        tool_call(13, "append_docs", {"content": "## Additional Notes\nMore info."})
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "append_docs appends successfully",
        responses,
        lambda r: "Documentation appended" in get_tool_result_text(r, 13),
    )

    # Test 14: get_docs after writes
    print("Test 14: get_docs after write + append")
    msgs = init_messages() + [tool_call(14, "get_docs", {})]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "get_docs contains both sections",
        responses,
        lambda r: "Test Setup" in get_tool_result_text(r, 14)
        and "Additional Notes" in get_tool_result_text(r, 14),
    )

    # Test 15: run_query - count
    print("Test 15: run_query - count")
    msgs = init_messages() + [
        tool_call(15, "run_query", {
            "instance_name": "local-test",
            "method": "POST",
            "path": "/test-logs-2024/_count",
            "body": {"query": {"match_all": {}}},
        })
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "Count returns count field",
        responses,
        lambda r: "count" in get_tool_result_text(r, 15),
    )

    # Test 16: run_query - get mapping
    print("Test 16: run_query - get mapping")
    msgs = init_messages() + [
        tool_call(16, "run_query", {
            "instance_name": "local-test",
            "method": "GET",
            "path": "/test-logs-2024/_mapping",
        })
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "Mapping shows properties",
        responses,
        lambda r: "properties" in get_tool_result_text(r, 16),
    )

    # Test 17: run_query - unknown instance
    print("Test 17: run_query - unknown instance")
    msgs = init_messages() + [
        tool_call(17, "run_query", {
            "instance_name": "nonexistent",
            "method": "GET",
            "path": "/_cluster/health",
        })
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "Unknown instance returns error",
        responses,
        lambda r: "Unknown instance" in get_tool_result_text(r, 17),
    )

    # Test 18: run_query - cat indices
    print("Test 18: run_query - cat indices")
    msgs = init_messages() + [
        tool_call(18, "run_query", {
            "instance_name": "local-test",
            "method": "GET",
            "path": "/_cat/indices?v&s=index",
        })
    ]
    responses = run_mcp_session(msgs, expected_responses=2)
    test(
        "Cat indices returns index list",
        responses,
        lambda r: "test-logs-2024" in get_tool_result_text(r, 18),
    )

    print(f"\n=== Results: {PASS} passed, {FAIL} failed ===")

    # Cleanup test artifacts
    for f in [
        os.path.join(SCRIPT_DIR, "memories", "memory_local-test.md"),
        os.path.join(SCRIPT_DIR, "docs.md"),
    ]:
        try:
            os.remove(f)
        except OSError:
            pass

    sys.exit(1 if FAIL > 0 else 0)


if __name__ == "__main__":
    main()
