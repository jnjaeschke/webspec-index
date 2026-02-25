"""Tests for webspec_index.lsp.hover."""

from webspec_index.lsp.hover import build_hover_content


class TestBuildHoverContent:
    def test_full_result(self):
        result = {
            "spec": "HTML",
            "anchor": "navigate",
            "title": "navigate",
            "type": "Algorithm",
            "content": "To **navigate** a navigable...",
        }
        md = build_hover_content(result)
        assert "## navigate" in md
        assert "*Algorithm*" in md
        assert "HTML#navigate" in md
        assert "To **navigate**" in md

    def test_minimal_result(self):
        result = {
            "spec": "HTML",
            "anchor": "some-section",
            "title": None,
            "type": "",
            "content": "",
        }
        md = build_hover_content(result)
        assert "some-section" in md

    def test_no_content(self):
        result = {
            "spec": "DOM",
            "anchor": "concept-tree",
            "title": "Trees",
            "type": "Heading",
            "content": "",
        }
        md = build_hover_content(result)
        assert "## Trees" in md
        assert "*Heading*" in md
        # No trailing content section
        assert md.count("\n\n") <= 2

    def test_title_fallback_to_anchor(self):
        result = {
            "spec": "HTML",
            "anchor": "my-anchor",
            "title": "",
            "type": "",
            "content": "Some content here.",
        }
        md = build_hover_content(result)
        assert "my-anchor" in md

    def test_content_only(self):
        result = {
            "spec": "HTML",
            "anchor": "test",
            "title": "Test Section",
            "type": "",
            "content": "Line one.\n\nLine two.",
        }
        md = build_hover_content(result)
        assert "## Test Section" in md
        assert "Line one." in md
        assert "Line two." in md
