"""Tests for query rule enforcement and result handling."""

import pytest

from elasticsearch_hub_mcp.tools import _is_read_allowed, _truncate_result


class TestReadAllowed:
    """Test the query rule enforcement logic."""

    def test_get_always_allowed(self):
        assert _is_read_allowed("GET", "/_cat/indices") is True
        assert _is_read_allowed("GET", "/my-index/_search") is True
        assert _is_read_allowed("GET", "/my-index/_doc/123") is True
        assert _is_read_allowed("GET", "/_cluster/health") is True

    def test_post_search_allowed(self):
        assert _is_read_allowed("POST", "/my-index/_search") is True
        assert _is_read_allowed("POST", "/my-index/_search?size=10") is True

    def test_post_count_allowed(self):
        assert _is_read_allowed("POST", "/my-index/_count") is True

    def test_post_msearch_allowed(self):
        assert _is_read_allowed("POST", "/_msearch") is True

    def test_post_mget_allowed(self):
        assert _is_read_allowed("POST", "/_mget") is True
        assert _is_read_allowed("POST", "/my-index/_mget") is True

    def test_post_field_caps_allowed(self):
        assert _is_read_allowed("POST", "/my-index/_field_caps") is True

    def test_post_cat_allowed(self):
        assert _is_read_allowed("POST", "/_cat/indices") is True
        assert _is_read_allowed("POST", "/_cat/shards") is True

    def test_post_cluster_allowed(self):
        assert _is_read_allowed("POST", "/_cluster/health") is True
        assert _is_read_allowed("POST", "/_cluster/stats") is True

    def test_post_mapping_allowed(self):
        assert _is_read_allowed("POST", "/my-index/_mapping") is True

    def test_post_validate_query_allowed(self):
        assert _is_read_allowed("POST", "/my-index/_validate/query") is True

    def test_put_blocked(self):
        assert _is_read_allowed("PUT", "/my-index") is False
        assert _is_read_allowed("PUT", "/my-index/_mapping") is False

    def test_delete_blocked(self):
        assert _is_read_allowed("DELETE", "/my-index") is False
        assert _is_read_allowed("DELETE", "/my-index/_doc/123") is False

    def test_post_index_blocked(self):
        assert _is_read_allowed("POST", "/my-index/_doc") is False
        assert _is_read_allowed("POST", "/my-index/_bulk") is False
        assert _is_read_allowed("POST", "/my-index/_update/123") is False

    def test_post_reindex_blocked(self):
        assert _is_read_allowed("POST", "/_reindex") is False

    def test_case_insensitive_method(self):
        assert _is_read_allowed("get", "/_cat/indices") is True
        assert _is_read_allowed("post", "/my-index/_search") is True
        assert _is_read_allowed("put", "/my-index") is False


class TestTruncateResult:
    def test_small_result_returned_inline(self):
        result = '{"hits": []}'
        assert _truncate_result("test", result) == result

    def test_large_result_written_to_file(self, tmp_path, monkeypatch):
        import elasticsearch_hub_mcp.tools as tools_module

        monkeypatch.setattr(tools_module, "TMP_DIR", tmp_path)

        large = "x" * 20_000
        result = _truncate_result("test-cluster", large)
        assert "Result exceeded 10 KB" in result
        assert "test-cluster_" in result
