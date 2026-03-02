"""Tests for the memory system."""

import json

import pytest

from elasticsearch_hub_mcp.memory import MemoryObject, get_memories, write_memory


def test_empty_memories(tmp_memories_dir):
    result = get_memories("test-instance")
    assert json.loads(result) == []


def test_write_and_read_memory(tmp_memories_dir):
    mem = MemoryObject(
        index="logs-2024",
        context="The timestamp field is @timestamp, not timestamp",
        type="lessons_learned",
    )
    result = write_memory("test-instance", mem)
    assert "Memory saved" in result

    memories = json.loads(get_memories("test-instance"))
    assert len(memories) == 1
    assert memories[0]["context"] == "The timestamp field is @timestamp, not timestamp"
    assert memories[0]["index"] == "logs-2024"
    assert memories[0]["type"] == "lessons_learned"
    assert memories[0]["id"]  # uuid generated
    assert memories[0]["created_at"]  # timestamp generated


def test_multiple_memories(tmp_memories_dir):
    for i in range(3):
        mem = MemoryObject(context=f"Memory {i}", type="info")
        write_memory("test-instance", mem)

    memories = json.loads(get_memories("test-instance"))
    assert len(memories) == 3


def test_large_memories_return_file_path(tmp_memories_dir):
    # Write enough data to exceed 10KB
    for i in range(100):
        mem = MemoryObject(
            context=f"{'x' * 200} item {i}",
            type="info",
        )
        write_memory("test-instance", mem)

    result = get_memories("test-instance")
    assert "Content limit exceeded" in result
    assert "memory_test-instance.json" in result


def test_separate_instance_memories(tmp_memories_dir):
    write_memory("instance-a", MemoryObject(context="A info", type="info"))
    write_memory("instance-b", MemoryObject(context="B info", type="info"))

    a_memories = json.loads(get_memories("instance-a"))
    b_memories = json.loads(get_memories("instance-b"))

    assert len(a_memories) == 1
    assert len(b_memories) == 1
    assert a_memories[0]["context"] == "A info"
    assert b_memories[0]["context"] == "B info"
