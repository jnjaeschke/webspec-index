"""Integration tests using paired fixtures and FixtureProvider.

Tests the full analysis pipeline: scanning → validation → coverage,
using frozen spec data instead of a live database.
"""

import json
from pathlib import Path

import pytest

from webspec_index.lsp.matcher import MatchResult
from webspec_index.lsp.server import SpecLensServer, _create_server

FIXTURES_DIR = Path(__file__).parent.parent / "fixtures" / "lsp"


class FixtureProvider:
    """Test provider that loads spec data from frozen JSON fixture files."""

    def __init__(self, fixture_dir: Path):
        self.specs: dict[str, dict] = {}
        for path in fixture_dir.glob("spec_*.json"):
            data = json.loads(path.read_text())
            key = f"{data['spec']}#{data['anchor']}"
            self.specs[key] = data

    def query(self, spec_anchor: str) -> dict:
        if spec_anchor not in self.specs:
            raise KeyError(f"No fixture for {spec_anchor}")
        return self.specs[spec_anchor]

    def spec_urls(self) -> list[dict]:
        seen: dict[str, dict] = {}
        for data in self.specs.values():
            name = data["spec"]
            if name not in seen:
                seen[name] = {
                    "spec": name,
                    "base_url": f"https://{name.lower()}.spec.whatwg.org",
                }
        return list(seen.values())


@pytest.fixture
def navigate_fixture():
    return FIXTURES_DIR / "navigate"


@pytest.fixture
def navigate_provider(navigate_fixture):
    return FixtureProvider(navigate_fixture)


@pytest.fixture
def navigate_input(navigate_fixture):
    return (navigate_fixture / "input.cpp").read_text()


@pytest.fixture
def navigate_expected(navigate_fixture):
    return json.loads((navigate_fixture / "expected.json").read_text())


@pytest.fixture
def navigate_server(navigate_provider):
    return SpecLensServer(provider=navigate_provider)


class TestFixtureProvider:
    def test_loads_spec(self, navigate_provider):
        result = navigate_provider.query("HTML#navigate")
        assert result["spec"] == "HTML"
        assert result["anchor"] == "navigate"
        assert result["type"] == "Algorithm"

    def test_spec_urls(self, navigate_provider):
        urls = navigate_provider.spec_urls()
        assert len(urls) == 1
        assert urls[0]["spec"] == "HTML"
        assert urls[0]["base_url"] == "https://html.spec.whatwg.org"

    def test_missing_spec_raises(self, navigate_provider):
        with pytest.raises(KeyError):
            navigate_provider.query("DOM#nonexistent")


class TestUrlScanning:
    def test_finds_spec_url(self, navigate_server, navigate_input):
        matches = navigate_server._scan_doc("file:///test.cpp", navigate_input, 1)
        assert len(matches) == 1
        assert matches[0].spec == "HTML"
        assert matches[0].anchor == "navigate"
        assert matches[0].line == 0

    def test_caches_scan_results(self, navigate_server, navigate_input):
        m1 = navigate_server._scan_doc("file:///test.cpp", navigate_input, 1)
        m2 = navigate_server._scan_doc("file:///test.cpp", navigate_input, 1)
        assert m1 is m2  # same object from cache


class TestHoverIntegration:
    def test_query_spec_returns_data(self, navigate_server, navigate_input):
        # Trigger scan first to initialize patterns
        navigate_server._scan_doc("file:///test.cpp", navigate_input, 1)
        result = navigate_server._query_spec("HTML", "navigate")
        assert result is not None
        assert result["title"] == "navigate"
        assert result["type"] == "Algorithm"

    def test_query_spec_caches(self, navigate_server, navigate_input):
        navigate_server._scan_doc("file:///test.cpp", navigate_input, 1)
        r1 = navigate_server._query_spec("HTML", "navigate")
        r2 = navigate_server._query_spec("HTML", "navigate")
        assert r1 is r2

    def test_query_unknown_returns_none(self, navigate_server, navigate_input):
        navigate_server._scan_doc("file:///test.cpp", navigate_input, 1)
        result = navigate_server._query_spec("HTML", "nonexistent")
        assert result is None


class TestStepValidation:
    def test_validates_steps(self, navigate_server, navigate_input, navigate_expected):
        validations = navigate_server._validate_doc("file:///test.cpp", navigate_input, 1)
        assert len(validations) == len(navigate_expected["validations"])

    def test_step_results_match_expected(self, navigate_server, navigate_input, navigate_expected):
        validations = navigate_server._validate_doc("file:///test.cpp", navigate_input, 1)
        for val, expected in zip(validations, navigate_expected["validations"]):
            assert val.step.line == expected["line"], f"Line mismatch for step {expected['step']}"
            assert val.step.number == expected["step"]
            assert val.result.value == expected["result"]

    def test_not_found_step(self, navigate_server, navigate_input):
        validations = navigate_server._validate_doc("file:///test.cpp", navigate_input, 1)
        step_99 = [v for v in validations if v.step.number == [99]]
        assert len(step_99) == 1
        assert step_99[0].result == MatchResult.NOT_FOUND

    def test_validation_caching(self, navigate_server, navigate_input):
        v1 = navigate_server._validate_doc("file:///test.cpp", navigate_input, 1)
        v2 = navigate_server._validate_doc("file:///test.cpp", navigate_input, 1)
        assert v1 is v2

    def test_different_version_revalidates(self, navigate_server, navigate_input):
        v1 = navigate_server._validate_doc("file:///test.cpp", navigate_input, 1)
        v2 = navigate_server._validate_doc("file:///test.cpp", navigate_input, 2)
        assert v1 is not v2


class TestCoverageIntegration:
    def test_computes_coverage(self, navigate_server, navigate_input, navigate_expected):
        coverages = navigate_server._coverage_doc("file:///test.cpp", navigate_input, 1)
        assert len(coverages) == 1
        url_match, cov = coverages[0]
        assert url_match.anchor == "navigate"

        expected_cov = navigate_expected["coverage"]
        assert cov.total_steps == expected_cov["total"]
        assert cov.implemented_count == expected_cov["implemented"]
        assert cov.missing == expected_cov["missing"]

    def test_coverage_caching(self, navigate_server, navigate_input):
        c1 = navigate_server._coverage_doc("file:///test.cpp", navigate_input, 1)
        c2 = navigate_server._coverage_doc("file:///test.cpp", navigate_input, 1)
        assert c1 is c2

    def test_no_steps_returns_empty(self, navigate_server):
        text = "// Just a plain file with no spec URLs\nint main() {}"
        coverages = navigate_server._coverage_doc("file:///plain.cpp", text, 1)
        assert len(coverages) == 0


class TestFuzzyThreshold:
    def test_default_threshold(self, navigate_provider):
        server = SpecLensServer(provider=navigate_provider)
        assert server.fuzzy_threshold == 0.85

    def test_custom_threshold_affects_matching(self, navigate_provider, navigate_input):
        # With a very high threshold, more things become mismatches
        server = SpecLensServer(provider=navigate_provider)
        server.fuzzy_threshold = 0.99
        strict_vals = server._validate_doc("file:///test.cpp", navigate_input, 1)

        server2 = SpecLensServer(provider=navigate_provider)
        server2.fuzzy_threshold = 0.5
        lenient_vals = server2._validate_doc("file:///test.cpp", navigate_input, 1)

        strict_mismatches = sum(1 for v in strict_vals if v.result == MatchResult.MISMATCH)
        lenient_mismatches = sum(1 for v in lenient_vals if v.result == MatchResult.MISMATCH)
        assert strict_mismatches >= lenient_mismatches


class TestCreateServer:
    def test_creates_server_with_provider(self, navigate_provider):
        server = _create_server(provider=navigate_provider)
        assert isinstance(server, SpecLensServer)
        assert server.provider is navigate_provider

    def test_creates_server_default_provider(self):
        server = _create_server()
        assert isinstance(server, SpecLensServer)
