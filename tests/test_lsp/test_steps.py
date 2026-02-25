"""Tests for webspec_index.lsp.steps."""

from webspec_index.lsp.steps import (
    AlgorithmStep,
    find_step,
    flatten_steps,
    parse_steps,
    strip_markdown,
)


class TestStripMarkdown:
    def test_bold(self):
        assert strip_markdown("**bold**") == "bold"

    def test_italic(self):
        assert strip_markdown("*italic*") == "italic"

    def test_code(self):
        assert strip_markdown("`code`") == "code"

    def test_link(self):
        assert strip_markdown("[text](https://example.com)") == "text"

    def test_mixed(self):
        result = strip_markdown("Let *x* be the result of [foo](https://bar.com)")
        assert result == "Let x be the result of foo"

    def test_nested_bold_link(self):
        # All patterns apply in sequence, so both link and bold are stripped
        result = strip_markdown("[**bold link**](url)")
        assert result == "bold link"


class TestParseSteps:
    def test_simple_flat(self):
        content = "1. First step.\n2. Second step.\n3. Third step."
        steps = parse_steps(content)
        assert len(steps) == 3
        assert steps[0].number == [1]
        assert steps[1].number == [2]
        assert steps[2].number == [3]
        assert "First step" in steps[0].text
        assert "Second step" in steps[1].text

    def test_nested_steps(self):
        content = (
            "1. Parent step.\n"
            "\n"
            "    1. Child one.\n"
            "    2. Child two.\n"
            "2. Next parent.\n"
        )
        steps = parse_steps(content)
        assert len(steps) == 2
        assert steps[0].number == [1]
        assert steps[1].number == [2]
        assert len(steps[0].children) == 2
        assert steps[0].children[0].number == [1, 1]
        assert steps[0].children[1].number == [1, 2]

    def test_deeply_nested(self):
        content = (
            "1. Top level.\n"
            "\n"
            "    1. Second level.\n"
            "\n"
            "        1. Third level.\n"
            "        2. Third level b.\n"
            "    2. Second level b.\n"
            "2. Top level b.\n"
        )
        steps = parse_steps(content)
        assert len(steps) == 2
        deep = steps[0].children[0].children[0]
        assert deep.number == [1, 1, 1]
        assert steps[0].children[0].children[1].number == [1, 1, 2]

    def test_preamble_ignored(self):
        content = (
            "To **navigate** a navigable:\n"
            "\n"
            "1. First actual step.\n"
            "2. Second step.\n"
        )
        steps = parse_steps(content)
        assert len(steps) == 2
        assert steps[0].number == [1]

    def test_notes_between_steps(self):
        content = (
            "1. Step one.\n"
            "\n"
            "    > **Note:** This is a note.\n"
            "    >\n"
            "    > More note text.\n"
            "2. Step two.\n"
        )
        steps = parse_steps(content)
        assert len(steps) == 2
        assert steps[0].number == [1]
        assert steps[1].number == [2]

    def test_markdown_stripped_from_text(self):
        content = '1. Let *cspNavigationType* be "`form-submission`".'
        steps = parse_steps(content)
        assert len(steps) == 1
        assert "cspNavigationType" in steps[0].text
        assert "*" not in steps[0].text

    def test_empty_content(self):
        assert parse_steps("") == []

    def test_no_steps(self):
        content = "Just a paragraph with no numbered list."
        assert parse_steps(content) == []

    def test_step_with_bullet_list(self):
        content = (
            "1. If all of the following are true:\n"
            "\n"
            "    * condition one;\n"
            "    * condition two;\n"
            "\n"
            "    then:\n"
            "\n"
            "    1. Do thing.\n"
            "    2. Return.\n"
            "2. Next step.\n"
        )
        steps = parse_steps(content)
        assert len(steps) == 2
        assert len(steps[0].children) == 2
        assert steps[0].children[0].number == [1, 1]


class TestFindStep:
    def test_find_top_level(self):
        steps = parse_steps("1. A.\n2. B.\n3. C.")
        assert find_step(steps, [2]).text == "B."

    def test_find_nested(self):
        content = "1. Parent.\n\n    1. Child.\n    2. Child b.\n2. Other."
        steps = parse_steps(content)
        step = find_step(steps, [1, 2])
        assert step is not None
        assert "Child b" in step.text

    def test_not_found(self):
        steps = parse_steps("1. A.\n2. B.")
        assert find_step(steps, [99]) is None

    def test_not_found_nested(self):
        steps = parse_steps("1. A.\n\n    1. Child.\n2. B.")
        assert find_step(steps, [1, 5]) is None

    def test_empty_number(self):
        steps = parse_steps("1. A.")
        assert find_step(steps, []) is None


class TestFlattenSteps:
    def test_flat(self):
        steps = parse_steps("1. A.\n2. B.\n3. C.")
        flat = flatten_steps(steps)
        assert len(flat) == 3
        assert [s.number for s in flat] == [[1], [2], [3]]

    def test_nested(self):
        content = "1. Parent.\n\n    1. Child.\n    2. Child b.\n2. Other."
        steps = parse_steps(content)
        flat = flatten_steps(steps)
        assert len(flat) == 4
        assert flat[0].number == [1]
        assert flat[1].number == [1, 1]
        assert flat[2].number == [1, 2]
        assert flat[3].number == [2]
