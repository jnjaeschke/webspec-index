"""Fuzzy text matching for step validation."""

from __future__ import annotations

import re
from enum import Enum


class MatchResult(Enum):
    EXACT = "exact"
    FUZZY = "fuzzy"
    MISMATCH = "mismatch"
    NOT_FOUND = "not_found"


_WHITESPACE_RE = re.compile(r'\s+')
_TRAILING_PUNCT_RE = re.compile(r'[.,:;!?]+$')


def normalize_text(text: str) -> str:
    """Normalize text for comparison.

    Strips markdown, collapses whitespace, lowercases, strips trailing punct.
    """
    from .steps import strip_markdown
    text = strip_markdown(text)
    text = _WHITESPACE_RE.sub(' ', text).strip()
    text = text.lower()
    text = _TRAILING_PUNCT_RE.sub('', text)
    return text


def jaro_winkler(s1: str, s2: str, prefix_weight: float = 0.1) -> float:
    """Pure-Python Jaro-Winkler similarity.

    Returns a value between 0.0 (no similarity) and 1.0 (identical).
    """
    if s1 == s2:
        return 1.0
    if not s1 or not s2:
        return 0.0

    len1, len2 = len(s1), len(s2)
    match_distance = max(len1, len2) // 2 - 1
    if match_distance < 0:
        match_distance = 0

    s1_matches = [False] * len1
    s2_matches = [False] * len2

    matches = 0
    transpositions = 0

    for i in range(len1):
        start = max(0, i - match_distance)
        end = min(i + match_distance + 1, len2)
        for j in range(start, end):
            if s2_matches[j] or s1[i] != s2[j]:
                continue
            s1_matches[i] = True
            s2_matches[j] = True
            matches += 1
            break

    if matches == 0:
        return 0.0

    k = 0
    for i in range(len1):
        if not s1_matches[i]:
            continue
        while not s2_matches[k]:
            k += 1
        if s1[i] != s2[k]:
            transpositions += 1
        k += 1

    jaro = (matches / len1 + matches / len2 + (matches - transpositions / 2) / matches) / 3

    # Winkler modification: boost for common prefix
    prefix = 0
    for i in range(min(4, len1, len2)):
        if s1[i] == s2[i]:
            prefix += 1
        else:
            break

    return jaro + prefix * prefix_weight * (1 - jaro)


def classify_match(
    comment_text: str,
    spec_text: str,
    threshold: float = 0.85,
) -> MatchResult:
    """Classify how well a step comment matches the spec text.

    Args:
        comment_text: The text from the code comment (after step number).
        spec_text: The text from the spec algorithm step.
        threshold: Jaro-Winkler threshold for fuzzy match.
    """
    if not comment_text.strip():
        # Step number only, no text to compare — counts as exact
        return MatchResult.EXACT

    norm_comment = normalize_text(comment_text)
    norm_spec = normalize_text(spec_text)

    if not norm_comment or not norm_spec:
        return MatchResult.EXACT if not norm_comment else MatchResult.MISMATCH

    if norm_comment == norm_spec:
        return MatchResult.EXACT

    # Prefix/substring match: comment is truncated version of spec or vice versa
    if norm_spec.startswith(norm_comment) or norm_comment.startswith(norm_spec):
        return MatchResult.FUZZY

    if norm_comment in norm_spec or norm_spec in norm_comment:
        return MatchResult.FUZZY

    similarity = jaro_winkler(norm_comment, norm_spec)
    if similarity >= threshold:
        return MatchResult.FUZZY

    return MatchResult.MISMATCH
