"""Shared fixtures for tests."""

import json
import os
from pathlib import Path

import pytest


@pytest.fixture
def tmp_config(tmp_path):
    """Create a temporary config file with test instances."""
    config = [
        {
            "name": "test-cluster",
            "url": "http://localhost:9200",
            "environment": "QA",
            "query_rule": "ONLY_READ_OPERATIONS",
            "index_patterns": ["logs-*", "metrics-*"],
            "credentials": {"type": "basic", "username": "elastic", "password": "test123"},
            "default_timeout": 10,
        },
        {
            "name": "dev-cluster",
            "url": "http://localhost:9201",
            "environment": "DEV",
            "query_rule": "ALL_ACCESS",
            "index_patterns": ["dev-*"],
            "credentials": {"type": "api_key", "api_key": "test-api-key"},
            "ssl": {"verify_certs": False},
            "default_timeout": 5,
        },
    ]
    config_path = tmp_path / "config.json"
    config_path.write_text(json.dumps(config))
    return config_path


@pytest.fixture
def tmp_memories_dir(tmp_path, monkeypatch):
    """Redirect memory storage to a temp directory."""
    import elasticsearch_hub_mcp.memory as mem_module

    memories_dir = tmp_path / "memories"
    memories_dir.mkdir()
    monkeypatch.setattr(mem_module, "MEMORIES_DIR", memories_dir)
    return memories_dir
