"""Tests for config loading and validation."""

import json
import os

import pytest

from elasticsearch_hub_mcp.config import (
    ApiKeyCredentials,
    BasicCredentials,
    ElasticsearchInstance,
    QueryRule,
    load_config,
)


def test_load_basic_config(tmp_config):
    instances = load_config(tmp_config)
    assert len(instances) == 2

    test = instances[0]
    assert test.name == "test-cluster"
    assert test.url == "http://localhost:9200"
    assert test.query_rule == QueryRule.ONLY_READ_OPERATIONS
    assert test.index_patterns == ["logs-*", "metrics-*"]
    assert isinstance(test.credentials, BasicCredentials)
    assert test.credentials.username == "elastic"
    assert test.credentials.password.get_secret_value() == "test123"
    assert test.default_timeout == 10

    dev = instances[1]
    assert dev.name == "dev-cluster"
    assert dev.query_rule == QueryRule.ALL_ACCESS
    assert isinstance(dev.credentials, ApiKeyCredentials)
    assert dev.credentials.api_key.get_secret_value() == "test-api-key"
    assert dev.ssl is not None
    assert dev.ssl.verify_certs is False


def test_env_var_substitution(tmp_path, monkeypatch):
    monkeypatch.setenv("TEST_ES_USER", "admin")
    monkeypatch.setenv("TEST_ES_PASS", "secret")

    config = [
        {
            "name": "env-test",
            "url": "http://localhost:9200",
            "environment": "QA",
            "query_rule": "ALL_ACCESS",
            "index_patterns": ["*"],
            "credentials": {
                "type": "basic",
                "username": "${TEST_ES_USER}",
                "password": "${TEST_ES_PASS}",
            },
        }
    ]
    path = tmp_path / "config.json"
    path.write_text(json.dumps(config))

    instances = load_config(path)
    assert instances[0].credentials.username == "admin"
    assert instances[0].credentials.password.get_secret_value() == "secret"


def test_missing_env_var(tmp_path):
    config = [
        {
            "name": "env-test",
            "url": "http://localhost:9200",
            "query_rule": "ALL_ACCESS",
            "index_patterns": ["*"],
            "credentials": {
                "type": "basic",
                "username": "${NONEXISTENT_VAR}",
                "password": "test",
            },
        }
    ]
    path = tmp_path / "config.json"
    path.write_text(json.dumps(config))

    with pytest.raises(ValueError, match="NONEXISTENT_VAR"):
        load_config(path)


def test_duplicate_instance_names(tmp_path):
    config = [
        {
            "name": "dupe",
            "url": "http://localhost:9200",
            "query_rule": "ALL_ACCESS",
            "index_patterns": ["*"],
            "environment": "QA",
            "credentials": {"type": "api_key", "api_key": "key1"},
        },
        {
            "name": "dupe",
            "url": "http://localhost:9201",
            "query_rule": "ALL_ACCESS",
            "index_patterns": ["*"],
            "environment": "QA",
            "credentials": {"type": "api_key", "api_key": "key2"},
        },
    ]
    path = tmp_path / "config.json"
    path.write_text(json.dumps(config))

    with pytest.raises(ValueError, match="Duplicate instance names"):
        load_config(path)


def test_config_file_not_found():
    with pytest.raises(FileNotFoundError):
        load_config("/nonexistent/config.json")


def test_invalid_json_array(tmp_path):
    path = tmp_path / "config.json"
    path.write_text('{"not": "an array"}')

    with pytest.raises(ValueError, match="JSON array"):
        load_config(path)


def test_password_not_in_repr(tmp_config):
    instances = load_config(tmp_config)
    cred = instances[0].credentials
    assert isinstance(cred, BasicCredentials)
    assert "test123" not in repr(cred)
