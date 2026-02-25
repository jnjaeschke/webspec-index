"""Tests for webspec_index.lsp.matcher."""

from webspec_index.lsp.matcher import (
    MatchResult,
    classify_match,
    jaro_winkler,
    normalize_text,
)


class TestNormalizeText:
    def test_strips_markdown(self):
        assert normalize_text("Let *x* be **y**") == "let x be y"

    def test_strips_code(self):
        assert normalize_text('the "`form-submission`" type') == 'the "form-submission" type'

    def test_collapses_whitespace(self):
        assert normalize_text("foo   bar\tbaz") == "foo bar baz"

    def test_lowercases(self):
        assert normalize_text("Assert: userInvolvement") == "assert: userinvolvement"

    def test_strips_trailing_punct(self):
        assert normalize_text("some text.") == "some text"
        assert normalize_text("some text...") == "some text"
        assert normalize_text("some text;") == "some text"

    def test_strips_links(self):
        result = normalize_text("[Assert](https://example.com): foo")
        assert result == "assert: foo"

    def test_empty_string(self):
        assert normalize_text("") == ""


class TestJaroWinkler:
    def test_identical(self):
        assert jaro_winkler("hello", "hello") == 1.0

    def test_empty_strings(self):
        assert jaro_winkler("", "") == 1.0
        assert jaro_winkler("hello", "") == 0.0
        assert jaro_winkler("", "hello") == 0.0

    def test_similar_strings(self):
        score = jaro_winkler("martha", "marhta")
        assert score > 0.9

    def test_different_strings(self):
        score = jaro_winkler("hello", "world")
        assert score < 0.5

    def test_prefix_boost(self):
        # Jaro-Winkler should give higher score than Jaro for common prefix
        score1 = jaro_winkler("navigation", "navigating")
        score2 = jaro_winkler("navigation", "xavigation")
        assert score1 > score2

    def test_single_char(self):
        assert jaro_winkler("a", "a") == 1.0
        assert jaro_winkler("a", "b") == 0.0


class TestClassifyMatch:
    def test_exact_match(self):
        result = classify_match(
            "Let cspNavigationType be form-submission",
            "Let *cspNavigationType* be `form-submission`",
        )
        assert result == MatchResult.EXACT

    def test_exact_with_surrounding_quotes(self):
        # Quotes around code spans remain after stripping, so this is fuzzy
        result = classify_match(
            "Let cspNavigationType be form-submission",
            "Let *cspNavigationType* be \"`form-submission`\"",
        )
        assert result == MatchResult.FUZZY

    def test_empty_comment_text(self):
        # Step number only, no text
        result = classify_match("", "Some spec text")
        assert result == MatchResult.EXACT

    def test_prefix_match(self):
        result = classify_match(
            "Let cspNavigationType be",
            "Let *cspNavigationType* be \"`form-submission`\" if *formDataEntryList* is non-null",
        )
        assert result == MatchResult.FUZZY

    def test_substring_match(self):
        result = classify_match(
            "Assert: userInvolvement is browser UI",
            "Assert: *userInvolvement* is \"browser UI\".",
        )
        assert result in (MatchResult.EXACT, MatchResult.FUZZY)

    def test_mismatch(self):
        result = classify_match(
            "Do something completely different",
            "Let x be the result of running foo",
        )
        assert result == MatchResult.MISMATCH

    def test_fuzzy_similar(self):
        result = classify_match(
            "Let source snapshot params be the result",
            "Let sourceSnapshotParams be the result",
        )
        # These are similar enough for fuzzy match
        assert result in (MatchResult.FUZZY, MatchResult.EXACT)

    def test_both_empty(self):
        result = classify_match("", "")
        assert result == MatchResult.EXACT

    def test_comment_only_whitespace(self):
        result = classify_match("   ", "Some text")
        assert result == MatchResult.EXACT  # treated as step-number-only
