"""Per-instance memory persistence system."""

import json
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Literal

from pydantic import BaseModel, Field

MEMORIES_DIR = Path(__file__).resolve().parent.parent.parent / "memories"
SIZE_LIMIT = 10 * 1024  # 10 KB


class MemoryObject(BaseModel):
    index: str | None = None
    context: str
    type: Literal["info", "lessons_learned"]
    created_at: str = ""
    id: str = ""


def _memory_file(instance_name: str) -> Path:
    return MEMORIES_DIR / f"memory_{instance_name}.json"


def get_memories(instance_name: str) -> str:
    """Read all memories for an instance. Returns inline JSON or file path notice."""
    MEMORIES_DIR.mkdir(parents=True, exist_ok=True)
    path = _memory_file(instance_name)

    if not path.exists():
        return json.dumps([], indent=2)

    content = path.read_text()
    if len(content) < SIZE_LIMIT:
        return content
    else:
        return (
            f"Content limit exceeded (10 KB). "
            f"Memory records at {path}, use command line tools to prevent context fill"
        )


def write_memory(instance_name: str, memory: MemoryObject) -> str:
    """Append a memory record to the instance's memory file."""
    MEMORIES_DIR.mkdir(parents=True, exist_ok=True)
    path = _memory_file(instance_name)

    # Load existing
    if path.exists():
        records = json.loads(path.read_text())
    else:
        records = []

    # Auto-generate id and timestamp
    entry = memory.model_dump()
    entry["id"] = str(uuid.uuid4())
    entry["created_at"] = datetime.now(timezone.utc).isoformat()

    records.append(entry)
    path.write_text(json.dumps(records, indent=2))

    return f"Memory saved for instance '{instance_name}' (id: {entry['id']})"
