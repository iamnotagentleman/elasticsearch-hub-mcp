"""Manages async HTTP sessions for Elasticsearch/OpenSearch clusters."""

import base64
import ssl

import aiohttp

from .config import (
    ApiKeyCredentials,
    BasicCredentials,
    ElasticsearchInstance,
)


class ConnectionManager:
    def __init__(self) -> None:
        self._sessions: dict[str, aiohttp.ClientSession] = {}
        self._instances: dict[str, ElasticsearchInstance] = {}

    async def initialize(self, instances: list[ElasticsearchInstance]) -> None:
        """Create aiohttp sessions for all configured instances."""
        for inst in instances:
            headers: dict[str, str] = {
                "Content-Type": "application/json",
                "Accept": "application/json",
            }

            # Credentials
            auth = None
            if isinstance(inst.credentials, BasicCredentials):
                auth = aiohttp.BasicAuth(
                    inst.credentials.username,
                    inst.credentials.password.get_secret_value(),
                )
            elif isinstance(inst.credentials, ApiKeyCredentials):
                headers["Authorization"] = (
                    f"ApiKey {inst.credentials.api_key.get_secret_value()}"
                )

            # SSL
            ssl_context: ssl.SSLContext | bool | None = None
            if inst.ssl:
                if not inst.ssl.verify_certs:
                    ssl_context = False  # disable verification
                elif inst.ssl.ca_certs:
                    ssl_context = ssl.create_default_context(cafile=inst.ssl.ca_certs)

            connector = aiohttp.TCPConnector(ssl=ssl_context)
            timeout = aiohttp.ClientTimeout(total=inst.default_timeout)

            self._sessions[inst.name] = aiohttp.ClientSession(
                base_url=inst.url,
                auth=auth,
                headers=headers,
                timeout=timeout,
                connector=connector,
            )
            self._instances[inst.name] = inst

    def get_session(self, name: str) -> aiohttp.ClientSession:
        if name not in self._sessions:
            available = ", ".join(sorted(self._sessions.keys()))
            raise KeyError(f"Unknown instance '{name}'. Available: {available}")
        return self._sessions[name]

    def get_instance_config(self, name: str) -> ElasticsearchInstance:
        if name not in self._instances:
            available = ", ".join(sorted(self._instances.keys()))
            raise KeyError(f"Unknown instance '{name}'. Available: {available}")
        return self._instances[name]

    def list_instances(self) -> list[ElasticsearchInstance]:
        return list(self._instances.values())

    async def close(self) -> None:
        for session in self._sessions.values():
            await session.close()
        self._sessions.clear()
        self._instances.clear()
