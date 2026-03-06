"""Tests for the memory system."""

import pytest

from elasticsearch_hub_mcp.memory import get_memories, write_memory


def test_empty_memories(tmp_memories_dir):
    result = get_memories("test-instance")
    assert result == "(No memories yet for this instance.)"


def test_write_and_read_memory(tmp_memories_dir):
    result = write_memory("test-instance", "The timestamp field is @timestamp, not timestamp")
    assert "Memory saved" in result

    memories = get_memories("test-instance")
    assert "The timestamp field is @timestamp, not timestamp" in memories


def test_multiple_memories(tmp_memories_dir):
    for i in range(3):
        write_memory("test-instance", f"Memory {i}")

    memories = get_memories("test-instance")
    for i in range(3):
        assert f"Memory {i}" in memories


def test_large_memories_return_file_path(tmp_memories_dir):
    # Write enough data to exceed 10KB
    for i in range(100):
        write_memory("test-instance", f"{'x' * 200} item {i}")

    result = get_memories("test-instance")
    assert "Content limit exceeded" in result
    assert "memory_test-instance.md" in result


def test_separate_instance_memories(tmp_memories_dir):
    write_memory("instance-a", "A info")
    write_memory("instance-b", "B info")

    a_memories = get_memories("instance-a")
    b_memories = get_memories("instance-b")

    assert "A info" in a_memories
    assert "B info" in b_memories
    assert "B info" not in a_memories
    assert "A info" not in b_memories
