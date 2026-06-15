"""Integration test for source-file analysis."""

import pytest

import webspec_index as wsi

pytestmark = pytest.mark.network

SOURCE = """\
// https://dom.spec.whatwg.org/#concept-tree
void WalkTree() {
  // Step 1. Do the first thing.
  first();
}
"""


def test_analyze_file(tmp_path):
    src = tmp_path / "tree.cc"
    src.write_text(SOURCE)

    results = wsi.analyze(str(src))
    assert isinstance(results, list)
    assert len(results) == 1

    fa = results[0]
    assert isinstance(fa, wsi.FileAnalysis)
    assert fa.file == str(src)
    assert fa.scopes, "expected at least one spec scope"

    scope = fa.scopes[0]
    assert isinstance(scope, wsi.ScopeAnalysis)
    assert scope.spec == "DOM"
    assert scope.anchor == "concept-tree"


def test_analyze_directory_recursive(tmp_path):
    (tmp_path / "sub").mkdir()
    (tmp_path / "sub" / "a.cc").write_text(SOURCE)
    (tmp_path / "plain.txt").write_text(SOURCE)  # non-source: ignored

    results = wsi.analyze(str(tmp_path), recursive=True)
    files = {fa.file for fa in results}
    assert str(tmp_path / "sub" / "a.cc") in files
    assert str(tmp_path / "plain.txt") not in files


def test_analyze_no_matches_returns_empty(tmp_path):
    src = tmp_path / "empty.cc"
    src.write_text("int main() { return 0; }\n")
    assert wsi.analyze(str(src)) == []
