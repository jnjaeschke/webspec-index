"""Hover content formatting."""

from __future__ import annotations


def build_hover_content(query_result: dict) -> str:
    """Format a query result as markdown for a hover tooltip.

    Args:
        query_result: Output of webspec_index.query()

    Returns:
        Markdown string for display in editor hover.
    """
    title = query_result.get("title", "")
    section_type = query_result.get("type", "")
    content = query_result.get("content", "")
    spec = query_result.get("spec", "")
    anchor = query_result.get("anchor", "")

    parts = []

    heading = title or anchor
    if heading:
        parts.append(f"## {heading}")

    if section_type:
        parts.append(f"*{section_type}* | {spec}#{anchor}")

    if content:
        parts.append(content)

    return "\n\n".join(parts)
