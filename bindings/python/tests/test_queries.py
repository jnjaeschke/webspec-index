"""Integration tests that fetch and index real spec data over the network.

These mirror the CLI's smoke tests. Spec data is cached locally after the first
run, so repeated runs are fast.
"""

import json

import pytest

import webspec_index as wsi

pytestmark = pytest.mark.network


def test_query_returns_typed_result():
    result = wsi.query("DOM#concept-tree")
    assert isinstance(result, wsi.QueryResult)
    assert result.spec == "DOM"
    assert result.anchor == "concept-tree"
    assert result.section_type == "definition"
    assert result.title
    assert isinstance(result.navigation, wsi.Navigation)


def test_query_to_dict_matches_json_and_uses_type_key():
    result = wsi.query("DOM#concept-tree")
    as_dict = result.to_dict()
    assert json.loads(result.to_json()) == as_dict
    # The JSON shape matches the CLI: section type is serialized under "type".
    assert "type" in as_dict
    assert as_dict["type"] == result.section_type
    assert as_dict["anchor"] == "concept-tree"


def test_query_url_form():
    result = wsi.query("https://dom.spec.whatwg.org/#concept-tree")
    assert result.spec == "DOM"
    assert result.anchor == "concept-tree"


def test_exists_true_and_false():
    found = wsi.exists("DOM#concept-tree")
    assert found.exists is True
    assert found.section_type is not None

    missing = wsi.exists("DOM#this-anchor-does-not-exist-xyz")
    assert missing.exists is False


def test_search_within_spec():
    result = wsi.search("tree order", spec="DOM", limit=5)
    assert isinstance(result, wsi.SearchResult)
    assert result.query == "tree order"
    assert len(result.results) <= 5
    for hit in result.results:
        assert hit.spec == "DOM"
        assert isinstance(hit.snippet, str)


def test_anchors_glob():
    result = wsi.anchors("concept-*", spec="DOM", limit=10)
    assert result.pattern == "concept-*"
    assert all(e.anchor.startswith("concept-") for e in result.results)


def test_list_headings():
    headings = wsi.list_headings("DOM")
    assert isinstance(headings, list)
    assert headings
    assert all(isinstance(h, wsi.ListEntry) for h in headings)


def test_refs_exact():
    result = wsi.refs("DOM#concept-tree", direction="outgoing", limit=5)
    assert result.direction == "outgoing"
    assert isinstance(result.matches, list)


def test_idl_query():
    result = wsi.idl("Node.nodeType", spec="DOM")
    assert isinstance(result, wsi.IdlResult)
    # The match set may evolve with the spec; just assert the shape.
    for entry in result.matches:
        assert isinstance(entry, wsi.IdlEntry)
        assert entry.canonical_name


def test_graph_structure():
    result = wsi.graph("DOM#concept-tree", direction="outgoing", max_depth=1, max_nodes=20)
    assert isinstance(result, wsi.GraphResult)
    assert result.root.spec == "DOM"
    assert result.root.anchor == "concept-tree"
    assert result.direction == "outgoing"
    for edge in result.edges:
        assert isinstance(edge.from_, str)
        assert isinstance(edge.to, str)


def test_query_raises_for_unknown_spec():
    with pytest.raises(wsi.WebspecError):
        wsi.query("NOT-A-REAL-SPEC-XYZ#whatever")
