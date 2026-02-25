"""Tests for webspec_index.lsp.scanner."""

import pytest
from webspec_index.lsp.scanner import (
    StepComment,
    UrlMatch,
    build_scopes,
    build_url_pattern,
    find_url_at_position,
    scan_document,
    scan_steps,
    build_spec_lookup,
)

SPEC_URLS = [
    {"spec": "HTML", "base_url": "https://html.spec.whatwg.org"},
    {"spec": "DOM", "base_url": "https://dom.spec.whatwg.org"},
    {"spec": "URL", "base_url": "https://url.spec.whatwg.org"},
]


@pytest.fixture
def pattern():
    return build_url_pattern(SPEC_URLS)


@pytest.fixture
def lookup():
    return build_spec_lookup(SPEC_URLS)


class TestBuildUrlPattern:
    def test_matches_html_url(self, pattern):
        m = pattern.search("https://html.spec.whatwg.org/#navigate")
        assert m is not None
        assert m.group(1) == "https://html.spec.whatwg.org"
        assert m.group(2) == "navigate"

    def test_matches_dom_url(self, pattern):
        m = pattern.search("https://dom.spec.whatwg.org/#concept-tree")
        assert m is not None
        assert m.group(2) == "concept-tree"

    def test_no_match_unknown_spec(self, pattern):
        m = pattern.search("https://example.com/#foo")
        assert m is None

    def test_no_match_without_fragment(self, pattern):
        m = pattern.search("https://html.spec.whatwg.org/")
        assert m is None

    def test_anchor_with_dots(self, pattern):
        m = pattern.search("https://html.spec.whatwg.org/#dom-element-click")
        assert m is not None
        assert m.group(2) == "dom-element-click"

    def test_anchor_with_colons(self, pattern):
        m = pattern.search("https://html.spec.whatwg.org/#concept-url-parser:percent-encoded-bytes")
        assert m is not None
        assert m.group(2) == "concept-url-parser:percent-encoded-bytes"

    def test_multipage_url(self, pattern):
        m = pattern.search("https://html.spec.whatwg.org/multipage/browsing-the-web.html#navigate")
        assert m is not None
        assert m.group(1) == "https://html.spec.whatwg.org"
        assert m.group(2) == "navigate"

    def test_multipage_url_deeper_path(self, pattern):
        m = pattern.search("https://html.spec.whatwg.org/multipage/parsing.html#parsing-main-inhead")
        assert m is not None
        assert m.group(2) == "parsing-main-inhead"


class TestScanDocument:
    def test_single_url_in_comment(self, pattern, lookup):
        text = "// https://html.spec.whatwg.org/#navigate"
        matches = scan_document(text, pattern, lookup)
        assert len(matches) == 1
        assert matches[0].spec == "HTML"
        assert matches[0].anchor == "navigate"
        assert matches[0].line == 0

    def test_multiple_urls(self, pattern, lookup):
        text = (
            "// https://html.spec.whatwg.org/#navigate\n"
            "code();\n"
            "// https://dom.spec.whatwg.org/#concept-tree\n"
        )
        matches = scan_document(text, pattern, lookup)
        assert len(matches) == 2
        assert matches[0].spec == "HTML"
        assert matches[0].line == 0
        assert matches[1].spec == "DOM"
        assert matches[1].line == 2

    def test_url_in_string(self, pattern, lookup):
        text = '  auto url = "https://html.spec.whatwg.org/#navigate";'
        matches = scan_document(text, pattern, lookup)
        assert len(matches) == 1
        assert matches[0].anchor == "navigate"

    def test_url_in_html_attribute(self, pattern, lookup):
        text = '<a href="https://html.spec.whatwg.org/#navigate">spec</a>'
        matches = scan_document(text, pattern, lookup)
        assert len(matches) == 1

    def test_no_urls(self, pattern, lookup):
        text = "just some code\nwith no spec urls\n"
        matches = scan_document(text, pattern, lookup)
        assert len(matches) == 0

    def test_col_positions(self, pattern, lookup):
        text = "  // https://html.spec.whatwg.org/#navigate"
        matches = scan_document(text, pattern, lookup)
        assert len(matches) == 1
        assert matches[0].col_start == 5
        assert matches[0].col_end == len(text)

    def test_two_urls_same_line(self, pattern, lookup):
        text = "// https://html.spec.whatwg.org/#navigate see also https://dom.spec.whatwg.org/#concept-tree"
        matches = scan_document(text, pattern, lookup)
        assert len(matches) == 2
        assert matches[0].spec == "HTML"
        assert matches[1].spec == "DOM"

    def test_python_comment(self, pattern, lookup):
        text = "# https://html.spec.whatwg.org/#navigate"
        matches = scan_document(text, pattern, lookup)
        assert len(matches) == 1

    def test_css_comment(self, pattern, lookup):
        text = "/* https://html.spec.whatwg.org/#navigate */"
        matches = scan_document(text, pattern, lookup)
        assert len(matches) == 1

    def test_multipage_url_in_code(self, pattern, lookup):
        text = "// https://html.spec.whatwg.org/multipage/browsing-the-web.html#navigate"
        matches = scan_document(text, pattern, lookup)
        assert len(matches) == 1
        assert matches[0].spec == "HTML"
        assert matches[0].anchor == "navigate"


class TestFindUrlAtPosition:
    def test_cursor_on_url(self, pattern, lookup):
        text = "// https://html.spec.whatwg.org/#navigate"
        matches = scan_document(text, pattern, lookup)
        result = find_url_at_position(matches, 0, 10)
        assert result is not None
        assert result.anchor == "navigate"

    def test_cursor_at_start(self, pattern, lookup):
        text = "// https://html.spec.whatwg.org/#navigate"
        matches = scan_document(text, pattern, lookup)
        result = find_url_at_position(matches, 0, 5)
        assert result is not None

    def test_cursor_at_end(self, pattern, lookup):
        text = "// https://html.spec.whatwg.org/#navigate"
        matches = scan_document(text, pattern, lookup)
        result = find_url_at_position(matches, 0, len(text))
        assert result is not None

    def test_cursor_before_url(self, pattern, lookup):
        text = "// https://html.spec.whatwg.org/#navigate"
        matches = scan_document(text, pattern, lookup)
        result = find_url_at_position(matches, 0, 0)
        assert result is None

    def test_cursor_wrong_line(self, pattern, lookup):
        text = "// https://html.spec.whatwg.org/#navigate\nfoo"
        matches = scan_document(text, pattern, lookup)
        result = find_url_at_position(matches, 1, 0)
        assert result is None


class TestScanSteps:
    def test_cpp_step_comment(self):
        text = "// Step 5.1. Assert: userInvolvement is browser UI"
        steps = scan_steps(text)
        assert len(steps) == 1
        assert steps[0].number == [5, 1]
        assert "Assert" in steps[0].text

    def test_step_without_prefix(self):
        text = "// 5.1. Let x be something"
        steps = scan_steps(text)
        assert len(steps) == 1
        assert steps[0].number == [5, 1]

    def test_step_no_trailing_dot(self):
        text = "// Step 5.1 Assert: foo"
        steps = scan_steps(text)
        assert len(steps) == 1
        assert steps[0].number == [5, 1]

    def test_step_number_only(self):
        text = "// Step 5."
        steps = scan_steps(text)
        assert len(steps) == 1
        assert steps[0].number == [5]
        assert steps[0].text == ""

    def test_python_step_comment(self):
        text = "# Step 3. Do something"
        steps = scan_steps(text)
        assert len(steps) == 1
        assert steps[0].number == [3]

    def test_css_step_comment(self):
        text = "/* Step 1. Init */"
        steps = scan_steps(text)
        assert len(steps) == 1
        assert steps[0].number == [1]
        assert steps[0].text == "Init"

    def test_no_step_comment(self):
        text = "// This is just a regular comment"
        steps = scan_steps(text)
        assert len(steps) == 0

    def test_multiple_steps(self):
        text = "// Step 1. First\n// Step 2. Second\n// Step 3. Third"
        steps = scan_steps(text)
        assert len(steps) == 3
        assert steps[0].line == 0
        assert steps[1].line == 1
        assert steps[2].line == 2

    def test_deeply_nested_number(self):
        text = "// Step 5.1.2 Deeply nested step"
        steps = scan_steps(text)
        assert len(steps) == 1
        assert steps[0].number == [5, 1, 2]

    def test_step_in_multiline_comment(self):
        text = "* Step 7. Something in block comment"
        steps = scan_steps(text)
        assert len(steps) == 1
        assert steps[0].number == [7]

    def test_asm_comment(self):
        text = "; Step 1. Assembly step"
        steps = scan_steps(text)
        assert len(steps) == 1
        assert steps[0].number == [1]

    def test_bare_number_not_matched(self):
        """Bare numbers without Step prefix, trailing dot, or multi-part format are rejected."""
        text = "// 42 is the answer to life"
        steps = scan_steps(text)
        assert len(steps) == 0

    def test_bare_number_with_port(self):
        """Port numbers in comments should not match as steps."""
        text = "// Use port 8080"
        steps = scan_steps(text)
        assert len(steps) == 0

    def test_single_number_with_trailing_dot(self):
        """Single number with trailing dot IS a step (e.g. '// 5. Let x be...')."""
        text = "// 5. Let x be something"
        steps = scan_steps(text)
        assert len(steps) == 1
        assert steps[0].number == [5]

    def test_multi_part_without_prefix_or_dot(self):
        """Multi-part number like 5.1 IS a step even without Step prefix or trailing dot."""
        text = "// 5.1 Let x be something"
        steps = scan_steps(text)
        assert len(steps) == 1
        assert steps[0].number == [5, 1]

    def test_multiline_continuation(self):
        """Continuation comment lines are appended to the step text."""
        text = "// Step 2.1 Foo Bar baz\n//       continues here"
        steps = scan_steps(text)
        assert len(steps) == 1
        assert steps[0].number == [2, 1]
        assert steps[0].text == "Foo Bar baz continues here"
        assert steps[0].line == 0

    def test_multiline_stops_at_next_step(self):
        """Continuation stops when a new step is found."""
        text = "// Step 1. First\n//   more first\n// Step 2. Second"
        steps = scan_steps(text)
        assert len(steps) == 2
        assert steps[0].text == "First more first"
        assert steps[1].text == "Second"

    def test_multiline_stops_at_non_comment(self):
        """Continuation stops at non-comment lines."""
        text = "// Step 1. First\ncode();\n// Step 2. Second"
        steps = scan_steps(text)
        assert len(steps) == 2
        assert steps[0].text == "First"
        assert steps[1].text == "Second"

    def test_multiline_python(self):
        """Multi-line works with Python comments."""
        text = "# Step 3. Do something\n#   important here"
        steps = scan_steps(text)
        assert len(steps) == 1
        assert steps[0].text == "Do something important here"


class TestBuildScopes:
    def test_single_url_with_steps(self, pattern, lookup):
        text = (
            "// https://html.spec.whatwg.org/#navigate\n"
            "// Step 1. First\n"
            "// Step 2. Second\n"
        )
        urls = scan_document(text, pattern, lookup)
        steps = scan_steps(text)
        scopes = build_scopes(urls, steps)
        assert len(scopes) == 1
        assert scopes[0][0].anchor == "navigate"
        assert len(scopes[0][1]) == 2

    def test_two_urls_split_steps(self, pattern, lookup):
        text = (
            "// https://html.spec.whatwg.org/#navigate\n"
            "// Step 1. From navigate\n"
            "// https://dom.spec.whatwg.org/#concept-tree\n"
            "// Step 1. From tree\n"
        )
        urls = scan_document(text, pattern, lookup)
        steps = scan_steps(text)
        scopes = build_scopes(urls, steps)
        assert len(scopes) == 2
        assert scopes[0][0].anchor == "navigate"
        assert len(scopes[0][1]) == 1
        assert scopes[1][0].anchor == "concept-tree"
        assert len(scopes[1][1]) == 1

    def test_steps_before_any_url(self, pattern, lookup):
        text = (
            "// Step 1. Orphan step\n"
            "// https://html.spec.whatwg.org/#navigate\n"
            "// Step 2. Assigned step\n"
        )
        urls = scan_document(text, pattern, lookup)
        steps = scan_steps(text)
        scopes = build_scopes(urls, steps)
        # Orphan step (line 0) has no preceding URL, so not assigned
        assert len(scopes) == 1
        assert len(scopes[0][1]) == 1
        assert scopes[0][1][0].number == [2]

    def test_no_urls(self, pattern, lookup):
        text = "// Step 1. Orphan"
        urls = scan_document(text, pattern, lookup)
        steps = scan_steps(text)
        scopes = build_scopes(urls, steps)
        assert len(scopes) == 0
