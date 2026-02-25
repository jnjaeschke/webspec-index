"""Algorithm step parsing from spec markdown content."""

from __future__ import annotations

import re
from dataclasses import dataclass, field


@dataclass
class AlgorithmStep:
    """A single step in a spec algorithm."""

    number: list[int]  # e.g., [5, 1] for step 5.1
    text: str  # plain text, markdown stripped
    children: list[AlgorithmStep] = field(default_factory=list)


# Matches a numbered list item: optional indentation, then "N. text"
_STEP_LINE_RE = re.compile(r'^( *)\d+\.\s')

# Markdown formatting to strip
_MD_LINK_RE = re.compile(r'\[([^\]]*)\]\([^)]*\)')
_MD_BOLD_RE = re.compile(r'\*\*([^*]*)\*\*')
_MD_ITALIC_RE = re.compile(r'\*([^*]*)\*')
_MD_CODE_RE = re.compile(r'`([^`]*)`')


def strip_markdown(text: str) -> str:
    """Strip markdown inline formatting, keeping the text content."""
    text = _MD_LINK_RE.sub(r'\1', text)
    text = _MD_BOLD_RE.sub(r'\1', text)
    text = _MD_ITALIC_RE.sub(r'\1', text)
    text = _MD_CODE_RE.sub(r'\1', text)
    return text


def _parse_step_line(line: str) -> tuple[int, int, str] | None:
    """Parse a numbered list line.

    Returns (indent_level, step_num, text) or None if not a step line.
    indent_level is in units of 4-space indentation.
    """
    m = _STEP_LINE_RE.match(line)
    if not m:
        return None
    spaces = len(m.group(1))
    indent = spaces // 4
    # Extract the number and text
    stripped = line[len(m.group(0)):].rstrip()
    rest = line.lstrip()
    # "N. text..." - extract N
    dot_pos = rest.index('.')
    num = int(rest[:dot_pos])
    text = rest[dot_pos + 1:].strip()
    return indent, num, text


def parse_steps(content: str) -> list[AlgorithmStep]:
    """Parse algorithm steps from markdown content.

    Expects the content field from webspec_index.query(), which contains
    numbered lists at various indentation levels representing algorithm steps.
    """
    lines = content.split('\n')
    # Collect all step lines with their raw data
    raw_steps: list[tuple[int, int, str, int]] = []  # (indent, num, text, line_idx)

    i = 0
    while i < len(lines):
        parsed = _parse_step_line(lines[i])
        if parsed is not None:
            indent, num, text = parsed
            # Accumulate continuation lines (non-empty, non-step lines at greater indent)
            j = i + 1
            while j < len(lines):
                next_line = lines[j]
                if not next_line.strip():
                    # Blank line — could be followed by sub-steps or continuation
                    j += 1
                    continue
                if _parse_step_line(next_line) is not None:
                    break
                # Check if this is a continuation (deeper indent or same indent but not a step)
                stripped = next_line.lstrip()
                next_indent = (len(next_line) - len(stripped))
                step_indent = indent * 4
                if next_indent > step_indent and not stripped.startswith('>') and not stripped.startswith('*'):
                    # Continuation of current step text (not a note/blockquote)
                    text += ' ' + stripped
                else:
                    break
                j += 1
            raw_steps.append((indent, num, text, i))
            i = j
        else:
            i += 1

    # Build hierarchical step numbers
    steps: list[AlgorithmStep] = []
    # Stack of (indent_level, step_list) for building hierarchy
    stack: list[tuple[int, list[AlgorithmStep]]] = [(-1, steps)]

    for indent, num, text, _ in raw_steps:
        plain_text = strip_markdown(text)
        step = AlgorithmStep(number=[], text=plain_text)

        # Pop stack until we find the parent level
        while len(stack) > 1 and stack[-1][0] >= indent:
            stack.pop()

        parent_steps = stack[-1][1]
        parent_indent = stack[-1][0]

        # Build number path: parent's number + this step's num
        if parent_steps and parent_steps is not steps:
            # We're a child of some step — find our parent step
            # Our parent is the last step at the level above us
            pass

        parent_steps.append(step)

        # Push this level for potential children
        stack.append((indent, step.children))

    # Now assign numbers by walking the tree
    _assign_numbers(steps, [])
    return steps


def _assign_numbers(steps: list[AlgorithmStep], prefix: list[int]) -> None:
    """Assign hierarchical step numbers based on tree position."""
    for i, step in enumerate(steps, 1):
        step.number = prefix + [i]
        _assign_numbers(step.children, step.number)


def find_step(steps: list[AlgorithmStep], number: list[int]) -> AlgorithmStep | None:
    """Find a step by its hierarchical number path.

    Args:
        steps: Top-level step list
        number: e.g. [5, 1] for step 5.1
    """
    if not number:
        return None
    current = steps
    target = None
    for n in number:
        if n < 1 or n > len(current):
            return None
        target = current[n - 1]
        current = target.children
    return target


def flatten_steps(steps: list[AlgorithmStep]) -> list[AlgorithmStep]:
    """Flatten a step tree into a list (depth-first)."""
    result = []
    for step in steps:
        result.append(step)
        result.extend(flatten_steps(step.children))
    return result
