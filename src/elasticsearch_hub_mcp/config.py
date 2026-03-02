"""Configuration models and loading for ES MCP server."""

import json
import os
import re
from enum import Enum
from pathlib import Path
from typing import Annotated, Literal

from pydantic import BaseModel, Field, SecretStr, model_validator


class QueryRule(str, Enum):
    ONLY_READ_OPERATIONS = "ONLY_READ_OPERATIONS"
    ALL_ACCESS = "ALL_ACCESS"


class BasicCredentials(BaseModel):
    type: Literal["basic"]
    username: str
    password: SecretStr = Field(repr=False)


class ApiKeyCredentials(BaseModel):
    type: Literal["api_key"]
    api_key: SecretStr = Field(repr=False)


Credentials = Annotated[
    BasicCredentials | ApiKeyCredentials,
    Field(discriminator="type"),
]


class SSLConfig(BaseModel):
    verify_certs: bool = True
    ca_certs: str | None = None


class ElasticsearchInstance(BaseModel):
    name: str
    url: str
    environment: str
    query_rule: QueryRule
    index_patterns: list[str]
    credentials: Credentials
    ssl: SSLConfig | None = None
    default_timeout: int = 30


ENV_VAR_PATTERN = re.compile(r"\$\{([^}]+)}")


def _resolve_env_vars(text: str) -> str:
    """Replace ${ENV_VAR} patterns with environment variable values."""

    def replacer(match: re.Match) -> str:
        var_name = match.group(1)
        value = os.environ.get(var_name)
        if value is None:
            raise ValueError(
                f"Environment variable '{var_name}' is not set "
                f"(referenced in config)"
            )
        return value

    return ENV_VAR_PATTERN.sub(replacer, text)


def load_config(path: str | Path | None = None) -> list[ElasticsearchInstance]:
    """Load and validate ES instance configuration.

    Path resolution: ES_MCP_CONFIG env var > explicit path > ./config.json
    """
    if path is None:
        path = os.environ.get("ES_MCP_CONFIG", "./config.json")

    config_path = Path(path)
    if not config_path.exists():
        raise FileNotFoundError(f"Config file not found: {config_path.resolve()}")

    raw = config_path.read_text()
    resolved = _resolve_env_vars(raw)
    data = json.loads(resolved)

    if not isinstance(data, list):
        raise ValueError("Config must be a JSON array of instance definitions")

    instances = [ElasticsearchInstance.model_validate(item) for item in data]

    # Enforce unique names
    names = [inst.name for inst in instances]
    if len(names) != len(set(names)):
        dupes = [n for n in names if names.count(n) > 1]
        raise ValueError(f"Duplicate instance names: {set(dupes)}")

    return instances
