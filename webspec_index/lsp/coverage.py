"""Coverage computation for spec algorithm step tracking."""

from __future__ import annotations

from dataclasses import dataclass, field

from .matcher import MatchResult
from .steps import AlgorithmStep, flatten_steps


@dataclass
class CoverageResult:
    """Coverage of a spec algorithm in source code."""

    anchor: str
    total_steps: int
    implemented: list[list[int]]  # step numbers found in code
    missing: list[list[int]]  # step numbers not found in code
    warnings: int  # count of MISMATCH or NOT_FOUND
    reordered: int  # count of out-of-order steps (total - LIS)

    @property
    def implemented_count(self) -> int:
        return len(self.implemented)

    def summary(self) -> str:
        """One-line summary for code lens display."""
        parts = [f"{self.anchor}: {self.implemented_count}/{self.total_steps} steps"]
        if self.warnings:
            parts.append(f"{self.warnings} warning{'s' if self.warnings != 1 else ''}")
        if self.reordered:
            parts.append(f"{self.reordered} reordered")
        return " | ".join(parts)


def _longest_increasing_subsequence_length(seq: list[int]) -> int:
    """Length of the longest strictly increasing subsequence.

    Uses patience sorting (O(n log n)).
    """
    if not seq:
        return 0
    import bisect
    tails: list[int] = []
    for val in seq:
        pos = bisect.bisect_left(tails, val)
        if pos == len(tails):
            tails.append(val)
        else:
            tails[pos] = val
    return len(tails)


def compute_coverage(
    validations: list,  # list[StepValidation] — avoid circular import
    algo_steps: list[AlgorithmStep],
    anchor: str,
) -> CoverageResult:
    """Compute coverage of an algorithm from step validations.

    Args:
        validations: StepValidation objects for steps in this algorithm's scope.
        algo_steps: Parsed algorithm steps from the spec.
        anchor: The algorithm anchor name.
    """
    flat = flatten_steps(algo_steps)
    total = len(flat)

    # Build lookup: step number tuple -> flat index
    step_to_idx: dict[tuple[int, ...], int] = {}
    all_numbers: set[tuple[int, ...]] = set()
    for i, s in enumerate(flat):
        key = tuple(s.number)
        step_to_idx[key] = i
        all_numbers.add(key)

    # Collect implemented steps and their spec-order indices
    implemented: list[list[int]] = []
    implemented_set: set[tuple[int, ...]] = set()
    spec_order_indices: list[int] = []
    warnings = 0

    for v in validations:
        key = tuple(v.step.number)
        if v.result in (MatchResult.EXACT, MatchResult.FUZZY):
            if key not in implemented_set:
                implemented.append(v.step.number)
                implemented_set.add(key)
                if key in step_to_idx:
                    spec_order_indices.append(step_to_idx[key])
        elif v.result == MatchResult.MISMATCH:
            # Mismatched but step exists — still "implemented" (with warning)
            if key not in implemented_set:
                implemented.append(v.step.number)
                implemented_set.add(key)
                if key in step_to_idx:
                    spec_order_indices.append(step_to_idx[key])
            warnings += 1
        elif v.result == MatchResult.NOT_FOUND:
            warnings += 1

    # Missing = all spec steps not in implemented set
    missing = [s.number for s in flat if tuple(s.number) not in implemented_set]

    # Order checking via LIS
    lis_len = _longest_increasing_subsequence_length(spec_order_indices)
    reordered = len(spec_order_indices) - lis_len

    return CoverageResult(
        anchor=anchor,
        total_steps=total,
        implemented=implemented,
        missing=missing,
        warnings=warnings,
        reordered=reordered,
    )
