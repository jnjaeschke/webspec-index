"""Document scanning for spec URLs and step comments."""

from __future__ import annotations

import re
from dataclasses import dataclass


@dataclass
class UrlMatch:
    """A spec URL found in a document."""

    line: int
    col_start: int
    col_end: int
    spec: str
    anchor: str
    url: str


@dataclass
class StepComment:
    """A step comment found in source code."""

    line: int
    col_start: int
    col_end: int
    number: list[int]  # e.g., [5, 1] for step 5.1
    text: str  # text after the step number
    end_line: int | None = None  # last line for multi-line comments (None = same as line)


# Matches step comments in various comment styles:
# // Step 5.1. text    // 5.1. text    # Step 5. text    /* Step 5 text */
#
# To avoid false positives on bare numbers (e.g. "// 42 is the answer"),
# we require at least one of:
#   - "Step" prefix (explicit intent)
#   - Multi-part number like 5.1 (unambiguous step reference)
#   - Trailing dot after number (e.g. "5." acts as step delimiter)
STEP_PATTERN = re.compile(
    r'(?://|#|;+|/\*+|\*)\s*'        # comment prefix
    r'([Ss]tep\s+)?'                  # optional "Step " prefix (group 1)
    r'(\d{1,3}(?:\.\d{1,3})*)'        # step number, max 3 digits per part (group 2)
    r'(\.)?'                          # optional trailing dot (group 3)
    r'\s*(.*?)\s*(?:\*/)?$'           # text, optional */ close (group 4)
)


def build_url_pattern(spec_urls: list[dict]) -> re.Pattern:
    """Build regex from known spec base URLs.

    Matches both single-page URLs (base/#anchor) and multipage URLs
    (base/multipage/page.html#anchor).

    Args:
        spec_urls: List of {"spec": "HTML", "base_url": "https://html.spec.whatwg.org"}
    """
    bases = [re.escape(s["base_url"]) for s in spec_urls]
    # Allow optional path segments between base URL and #anchor
    # e.g. /multipage/browsing-the-web.html#navigate
    pattern = rf'({"|".join(bases)})/(?:[^\s#]*)?#([\w:._%{{}}()-]+)'
    return re.compile(pattern)


def build_spec_lookup(spec_urls: list[dict]) -> dict[str, str]:
    """Build base_url -> spec name lookup."""
    return {s["base_url"]: s["spec"] for s in spec_urls}


def scan_document(
    text: str, pattern: re.Pattern, spec_lookup: dict[str, str]
) -> list[UrlMatch]:
    """Scan document text for spec URLs.

    Returns list of UrlMatch sorted by (line, col_start).
    """
    matches = []
    for line_num, line in enumerate(text.splitlines()):
        for m in pattern.finditer(line):
            base_url = m.group(1)
            anchor = m.group(2)
            spec = spec_lookup.get(base_url, "")
            matches.append(
                UrlMatch(
                    line=line_num,
                    col_start=m.start(),
                    col_end=m.end(),
                    spec=spec,
                    anchor=anchor,
                    url=m.group(0),
                )
            )
    return matches


_CONTINUATION_RE = re.compile(
    r'\s*(?://|#|;+|\*)\s*(.*?)\s*(?:\*/)?$'
)


def scan_steps(text: str) -> list[StepComment]:
    """Scan document text for step comments.

    Supports multi-line comments: continuation lines (comment lines without
    a step number) immediately following a step comment are appended to its text.

    Returns list of StepComment sorted by line number.
    """
    results = []
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        m = STEP_PATTERN.search(lines[i])
        if m:
            has_step_prefix = m.group(1) is not None
            number_str = m.group(2)
            has_trailing_dot = m.group(3) is not None
            step_text = m.group(4)
            is_multi_part = '.' in number_str

            # Require at least one signal that this is a step reference
            if not (has_step_prefix or has_trailing_dot or is_multi_part):
                i += 1
                continue

            # Collect continuation lines
            col_end = m.end()
            j = i + 1
            while j < len(lines):
                # Stop if the next line is itself a step
                if STEP_PATTERN.search(lines[j]):
                    break
                cont = _CONTINUATION_RE.match(lines[j])
                if cont and cont.group(1):
                    step_text += " " + cont.group(1)
                    col_end = cont.end()
                    j += 1
                else:
                    break

            end_line = j - 1 if j > i + 1 else None
            number = [int(p) for p in number_str.split('.')]
            results.append(
                StepComment(
                    line=i,
                    col_start=m.start(),
                    col_end=col_end,
                    number=number,
                    text=step_text,
                    end_line=end_line,
                )
            )
            i = j
        else:
            i += 1
    return results


def find_url_at_position(
    matches: list[UrlMatch], line: int, col: int
) -> UrlMatch | None:
    """Find a URL match at the given cursor position."""
    for m in matches:
        if m.line == line and m.col_start <= col <= m.col_end:
            return m
    return None


def build_scopes(
    url_matches: list[UrlMatch], step_comments: list[StepComment]
) -> list[tuple[UrlMatch, list[StepComment]]]:
    """Associate step comments with their nearest preceding spec URL.

    A spec URL opens a scope that extends until the next spec URL or EOF.
    Each step comment is assigned to the nearest preceding URL.

    Returns list of (url_match, [step_comments]) pairs.
    """
    if not url_matches:
        return []

    # Sort URLs by line
    sorted_urls = sorted(url_matches, key=lambda u: u.line)
    sorted_steps = sorted(step_comments, key=lambda s: s.line)

    scopes: list[tuple[UrlMatch, list[StepComment]]] = []
    for url in sorted_urls:
        scopes.append((url, []))

    # Assign each step to the nearest preceding URL
    for step in sorted_steps:
        best_scope = None
        for i, (url, _) in enumerate(scopes):
            if url.line <= step.line:
                best_scope = i
            else:
                break
        if best_scope is not None:
            scopes[best_scope][1].append(step)

    return scopes
