"""
webspec-index: Query WHATWG/W3C web specifications

Provides three interfaces:
1. CLI: `webspec-index query HTML#navigate`
2. MCP Server: `webspec-index mcp`
3. Library: `import webspec_index; webspec_index.query("HTML#navigate")`
"""

import json
from typing import Optional
from ._webspec_index import (
    query as _query,
    search as _search,
    exists as _exists,
    anchors as _anchors,
    list_headings as _list_headings,
    refs as _refs,
    update as _update,
    spec_urls as _spec_urls,
    clear_db as _clear_db,
)

# Version is set by maturin from Cargo.toml
try:
    from importlib.metadata import version
    __version__ = version("webspec-index")
except Exception:
    __version__ = "unknown"

__all__ = [
    "query",
    "search",
    "exists",
    "anchors",
    "list_headings",
    "refs",
    "update",
    "spec_urls",
    "clear_db",
]


def _parse_json(result: str) -> dict:
    """Parse JSON result from Rust FFI"""
    return json.loads(result)


def query(spec_anchor: str, sha: Optional[str] = None) -> dict:
    """Query a specific section in a spec

    Args:
        spec_anchor: Spec and anchor in format "SPEC#anchor" (e.g., "HTML#navigate")
        sha: Optional commit SHA to query specific version

    Returns:
        dict with section info, navigation, children, and references

    Example:
        >>> result = query("HTML#navigate")
        >>> print(result["title"])
        navigate
        >>> print(result["section_type"])
        Algorithm
    """
    return _parse_json(_query(spec_anchor, sha))


def search(query_text: str, spec: Optional[str] = None, limit: int = 20) -> dict:
    """Search for text across all specs

    Args:
        query_text: Text to search for
        spec: Optional spec name to limit search
        limit: Maximum number of results (default 20)

    Returns:
        dict with list of matching sections and snippets

    Example:
        >>> results = search("tree order", spec="DOM", limit=5)
        >>> for result in results["results"]:
        ...     print(f"{result['spec']}#{result['anchor']}: {result['snippet']}")
    """
    return _parse_json(_search(query_text, spec, limit))


def exists(spec_anchor: str) -> bool:
    """Check if a section exists

    Args:
        spec_anchor: Spec and anchor in format "SPEC#anchor"

    Returns:
        True if section exists, False otherwise

    Example:
        >>> exists("HTML#navigate")
        True
        >>> exists("HTML#nonexistent")
        False
    """
    result = _parse_json(_exists(spec_anchor))
    return result["exists"]


def anchors(pattern: str, spec: Optional[str] = None, limit: int = 50) -> list[str]:
    """Find anchors matching a pattern

    Args:
        pattern: Glob pattern (e.g., "*-tree", "concept-*")
        spec: Optional spec name to limit search
        limit: Maximum number of results (default 50)

    Returns:
        List of dicts with anchor information

    Example:
        >>> results = anchors("*-tree", spec="DOM")
        >>> for anchor in results:
        ...     print(f"{anchor['spec']}#{anchor['anchor']}: {anchor['title']}")
    """
    result = _parse_json(_anchors(pattern, spec, limit))
    return result["results"]


def list_headings(spec: str, sha: Optional[str] = None) -> list[dict]:
    """List all headings in a spec

    Args:
        spec: Spec name (e.g., "HTML", "DOM")
        sha: Optional commit SHA for specific version

    Returns:
        List of heading entries with title, anchor, and depth

    Example:
        >>> headings = list_headings("DOM")
        >>> for h in headings:
        ...     indent = "  " * h["depth"]
        ...     print(f"{indent}{h['title']} (#{h['anchor']})")
    """
    return _parse_json(_list_headings(spec, sha))


def refs(
    spec_anchor: str,
    direction: str = "both",
    sha: Optional[str] = None
) -> dict:
    """Get references for a section

    Args:
        spec_anchor: Spec and anchor in format "SPEC#anchor"
        direction: "incoming", "outgoing", or "both" (default: "both")
        sha: Optional commit SHA for specific version

    Returns:
        dict with incoming and/or outgoing references

    Example:
        >>> refs_result = refs("HTML#navigate", direction="incoming")
        >>> for ref in refs_result["incoming"]:
        ...     print(f"{ref['spec']}#{ref['anchor']}")
    """
    return _parse_json(_refs(spec_anchor, direction, sha))


def update(spec: Optional[str] = None, force: bool = False) -> dict:
    """Update specs (fetch latest versions)

    Args:
        spec: Optional spec name to update (updates all if None)
        force: Force update even if recently checked (default: False)

    Returns:
        List of tuples (spec_name, snapshot_id or None)
        None indicates spec was already up to date

    Example:
        >>> result = update(spec="HTML")
        >>> for spec_name, snapshot_id in result:
        ...     if snapshot_id:
        ...         print(f"Updated {spec_name} (snapshot {snapshot_id})")
        ...     else:
        ...         print(f"{spec_name} already up to date")
    """
    return _parse_json(_update(spec, force))


def spec_urls() -> list[dict]:
    """Return known spec base URLs

    Returns:
        List of dicts with spec name and base URL

    Example:
        >>> urls = spec_urls()
        >>> for s in urls:
        ...     print(f"{s['spec']}: {s['base_url']}")
    """
    return _parse_json(_spec_urls())


def clear_db() -> str:
    """Clear the database (remove all indexed data)

    Returns:
        Path to the deleted database file

    Example:
        >>> path = clear_db()
        >>> print(f"Database cleared: {path}")
    """
    return _clear_db()
