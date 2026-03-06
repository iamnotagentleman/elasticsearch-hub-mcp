"""Per-instance memory persistence system."""

from pathlib import Path

MEMORIES_DIR = Path(__file__).resolve().parent.parent.parent / "memories"
SIZE_LIMIT = 10 * 1024  # 10 KB


def _memory_file(instance_name: str) -> Path:
    return MEMORIES_DIR / f"memory_{instance_name}.md"


def get_memories(instance_name: str) -> str:
    """Read all memories for an instance. Returns content or file path notice."""
    MEMORIES_DIR.mkdir(parents=True, exist_ok=True)
    path = _memory_file(instance_name)

    if not path.exists():
        return "(No memories yet for this instance.)"

    content = path.read_text()
    if len(content) < SIZE_LIMIT:
        return content
    else:
        return (
            f"Content limit exceeded (10 KB). "
            f"Memory records at {path}, use command line tools to prevent context fill"
        )


def write_memory(instance_name: str, content: str) -> str:
    """Append a memory entry to the instance's memory file."""
    MEMORIES_DIR.mkdir(parents=True, exist_ok=True)
    path = _memory_file(instance_name)

    if path.exists():
        existing = path.read_text()
        path.write_text(existing + "\n" + content)
    else:
        path.write_text(content)

    return f"Memory saved for instance '{instance_name}'."
