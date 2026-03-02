"""Global documentation read/write system."""

from pathlib import Path

DOCS_FILE = Path(__file__).resolve().parent.parent.parent / "docs.md"


def get_docs() -> str:
    """Read the global docs file."""
    if not DOCS_FILE.exists():
        return "(No documentation yet. Use write_docs to add setup info.)"
    return DOCS_FILE.read_text()


def write_docs(content: str) -> str:
    """Write or append to the global docs file."""
    if DOCS_FILE.exists():
        existing = DOCS_FILE.read_text()
        DOCS_FILE.write_text(existing + "\n" + content)
    else:
        DOCS_FILE.write_text(content)
    return "Documentation updated."
