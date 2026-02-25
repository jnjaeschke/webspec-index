"""Tests for webspec_index.lsp.coverage."""

from webspec_index.lsp.coverage import (
    CoverageResult,
    _longest_increasing_subsequence_length,
    compute_coverage,
)
from webspec_index.lsp.matcher import MatchResult
from webspec_index.lsp.scanner import StepComment
from webspec_index.lsp.steps import parse_steps


# Reuse StepValidation from server to avoid circular import issues
class _FakeValidation:
    """Minimal stand-in for StepValidation."""

    def __init__(self, number, result, text="", algo="test"):
        self.step = StepComment(
            line=0, col_start=0, col_end=10, number=number, text=text
        )
        self.result = result
        self.spec_text = text
        self.algo_name = algo


SIMPLE_ALGO = "1. First.\n2. Second.\n3. Third."
NESTED_ALGO = (
    "1. Parent.\n"
    "\n"
    "    1. Child one.\n"
    "    2. Child two.\n"
    "2. Other.\n"
)


class TestLIS:
    def test_empty(self):
        assert _longest_increasing_subsequence_length([]) == 0

    def test_single(self):
        assert _longest_increasing_subsequence_length([5]) == 1

    def test_sorted(self):
        assert _longest_increasing_subsequence_length([1, 2, 3, 4, 5]) == 5

    def test_reverse(self):
        assert _longest_increasing_subsequence_length([5, 4, 3, 2, 1]) == 1

    def test_mixed(self):
        # [1, 3, 2, 5] → LIS is [1, 2, 5] or [1, 3, 5], length 3
        assert _longest_increasing_subsequence_length([1, 3, 2, 5]) == 3

    def test_duplicates(self):
        # Strictly increasing, so duplicates don't extend
        assert _longest_increasing_subsequence_length([1, 1, 1]) == 1

    def test_longer_sequence(self):
        # [3, 1, 4, 1, 5, 9, 2, 6] → LIS is [1, 4, 5, 9] or [1, 2, 6], length 4
        assert _longest_increasing_subsequence_length([3, 1, 4, 1, 5, 9, 2, 6]) == 4


class TestComputeCoverage:
    def test_all_exact(self):
        steps = parse_steps(SIMPLE_ALGO)
        vals = [
            _FakeValidation([1], MatchResult.EXACT),
            _FakeValidation([2], MatchResult.EXACT),
            _FakeValidation([3], MatchResult.EXACT),
        ]
        cov = compute_coverage(vals, steps, "test")
        assert cov.total_steps == 3
        assert cov.implemented_count == 3
        assert cov.missing == []
        assert cov.warnings == 0
        assert cov.reordered == 0

    def test_partial_coverage(self):
        steps = parse_steps(SIMPLE_ALGO)
        vals = [
            _FakeValidation([1], MatchResult.EXACT),
            _FakeValidation([3], MatchResult.FUZZY),
        ]
        cov = compute_coverage(vals, steps, "test")
        assert cov.total_steps == 3
        assert cov.implemented_count == 2
        assert cov.missing == [[2]]
        assert cov.warnings == 0

    def test_mismatch_counts_as_implemented_with_warning(self):
        steps = parse_steps(SIMPLE_ALGO)
        vals = [
            _FakeValidation([1], MatchResult.EXACT),
            _FakeValidation([2], MatchResult.MISMATCH),
        ]
        cov = compute_coverage(vals, steps, "test")
        assert cov.implemented_count == 2
        assert cov.warnings == 1
        assert cov.missing == [[3]]

    def test_not_found_is_warning_only(self):
        steps = parse_steps(SIMPLE_ALGO)
        vals = [
            _FakeValidation([1], MatchResult.EXACT),
            _FakeValidation([99], MatchResult.NOT_FOUND),
        ]
        cov = compute_coverage(vals, steps, "test")
        assert cov.implemented_count == 1
        assert cov.warnings == 1
        assert len(cov.missing) == 2  # steps 2 and 3

    def test_reordered_detection(self):
        steps = parse_steps(SIMPLE_ALGO)
        # Steps implemented in reverse order: 3, 1, 2
        vals = [
            _FakeValidation([3], MatchResult.EXACT),
            _FakeValidation([1], MatchResult.EXACT),
            _FakeValidation([2], MatchResult.EXACT),
        ]
        cov = compute_coverage(vals, steps, "test")
        assert cov.implemented_count == 3
        # LIS of [2, 0, 1] is length 2 (e.g., [0, 1]), so reordered = 3 - 2 = 1
        assert cov.reordered == 1

    def test_no_validations(self):
        steps = parse_steps(SIMPLE_ALGO)
        cov = compute_coverage([], steps, "test")
        assert cov.total_steps == 3
        assert cov.implemented_count == 0
        assert len(cov.missing) == 3
        assert cov.warnings == 0
        assert cov.reordered == 0

    def test_nested_coverage(self):
        steps = parse_steps(NESTED_ALGO)
        # Total: [1], [1,1], [1,2], [2] = 4 steps
        vals = [
            _FakeValidation([1], MatchResult.EXACT),
            _FakeValidation([1, 2], MatchResult.FUZZY),
        ]
        cov = compute_coverage(vals, steps, "test")
        assert cov.total_steps == 4
        assert cov.implemented_count == 2
        assert [1, 1] in cov.missing
        assert [2] in cov.missing

    def test_duplicate_step_counted_once(self):
        steps = parse_steps(SIMPLE_ALGO)
        vals = [
            _FakeValidation([1], MatchResult.EXACT),
            _FakeValidation([1], MatchResult.EXACT),  # duplicate
            _FakeValidation([2], MatchResult.EXACT),
        ]
        cov = compute_coverage(vals, steps, "test")
        assert cov.implemented_count == 2
        assert cov.missing == [[3]]


class TestCoverageResult:
    def test_summary_all_good(self):
        cov = CoverageResult(
            anchor="navigate",
            total_steps=23,
            implemented=[[i] for i in range(1, 24)],
            missing=[],
            warnings=0,
            reordered=0,
        )
        assert cov.summary() == "navigate: 23/23 steps"

    def test_summary_with_warnings(self):
        cov = CoverageResult(
            anchor="navigate",
            total_steps=23,
            implemented=[[1], [2], [3]],
            missing=[[i] for i in range(4, 24)],
            warnings=2,
            reordered=0,
        )
        assert cov.summary() == "navigate: 3/23 steps | 2 warnings"

    def test_summary_with_reordered(self):
        cov = CoverageResult(
            anchor="navigate",
            total_steps=10,
            implemented=[[1], [2], [3]],
            missing=[],
            warnings=0,
            reordered=1,
        )
        assert cov.summary() == "navigate: 3/10 steps | 1 reordered"

    def test_summary_with_all(self):
        cov = CoverageResult(
            anchor="navigate",
            total_steps=23,
            implemented=[[1], [2]],
            missing=[],
            warnings=1,
            reordered=2,
        )
        assert cov.summary() == "navigate: 2/23 steps | 1 warning | 2 reordered"

    def test_summary_singular_warning(self):
        cov = CoverageResult(
            anchor="test",
            total_steps=5,
            implemented=[[1]],
            missing=[],
            warnings=1,
            reordered=0,
        )
        assert "1 warning" in cov.summary()
        assert "warnings" not in cov.summary()
