"""Offline smoke tests — no network access required."""

import pytest

import webspec_index as wsi


def test_version_is_string():
    assert isinstance(wsi.__version__, str)
    assert wsi.__version__


def test_parse_anchor_spec_form():
    assert wsi.parse_anchor("HTML#navigate") == ("HTML", "navigate", None)


def test_parse_anchor_url_form():
    spec, anchor, base_url = wsi.parse_anchor("https://dom.spec.whatwg.org/#concept-tree")
    assert spec == "DOM"
    assert anchor == "concept-tree"
    assert base_url == "https://dom.spec.whatwg.org"


def test_parse_anchor_invalid_raises():
    with pytest.raises(wsi.WebspecError):
        wsi.parse_anchor("no-hash-here")


def test_specs_returns_entries():
    entries = wsi.specs()
    assert isinstance(entries, list)
    assert entries, "expected at least one seeded spec"
    first = entries[0]
    assert isinstance(first, wsi.SpecUrlEntry)
    assert first.spec
    assert first.base_url.startswith("http")


def test_webspec_error_is_exception_subclass():
    assert issubclass(wsi.WebspecError, Exception)


def test_result_classes_are_exported():
    for name in (
        "QueryResult",
        "SearchResult",
        "AnchorsResult",
        "RefsResult",
        "IdlResult",
        "GraphResult",
        "PrDiffResult",
        "FileAnalysis",
    ):
        assert hasattr(wsi, name), name


def test_specs_entry_is_frozen():
    entry = wsi.specs()[0]
    with pytest.raises(AttributeError):
        entry.spec = "mutated"
