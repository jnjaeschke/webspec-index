"""webspec-lens LSP server using pygls."""

from __future__ import annotations

import asyncio
import logging
from dataclasses import dataclass
from typing import Protocol, runtime_checkable

from lsprotocol import types as lsp
from pygls.lsp.server import LanguageServer

from .coverage import CoverageResult, compute_coverage
from .hover import build_hover_content
from .matcher import MatchResult, classify_match
from .scanner import (
    StepComment, UrlMatch,
    build_scopes, build_url_pattern, find_url_at_position,
    scan_document, scan_steps, build_spec_lookup,
)
from .steps import AlgorithmStep, find_step, parse_steps

logger = logging.getLogger(__name__)

_DEBOUNCE_DELAY = 0.3  # seconds


@runtime_checkable
class SpecProvider(Protocol):
    """Interface for spec data access (allows DI for testing)."""

    def query(self, spec_anchor: str) -> dict: ...
    def spec_urls(self) -> list[dict]: ...


class WebspecProvider:
    """Production provider: calls webspec_index directly."""

    def query(self, spec_anchor: str) -> dict:
        import webspec_index
        return webspec_index.query(spec_anchor)

    def spec_urls(self) -> list[dict]:
        import webspec_index
        return webspec_index.spec_urls()


@dataclass
class StepValidation:
    """Result of validating a step comment against the spec."""

    step: StepComment
    result: MatchResult
    spec_text: str  # expected text from spec (empty if not found)
    algo_name: str  # algorithm name (anchor)


class SpecLensServer(LanguageServer):
    """LSP server with spec-aware hover and step validation."""

    def __init__(self, provider: SpecProvider | None = None):
        super().__init__("webspec-lens", "v0.1.0")
        self.provider = provider or WebspecProvider()
        self.fuzzy_threshold: float = 0.85
        # Caches
        self._url_pattern = None
        self._base_url_lookup: dict[str, str] = {}
        self._doc_urls: dict[str, tuple[int, list[UrlMatch]]] = {}  # uri -> (version, matches)
        self._query_cache: dict[str, dict] = {}  # spec#anchor -> result
        self._algo_steps_cache: dict[str, list[AlgorithmStep]] = {}  # anchor -> parsed steps
        self._doc_validations: dict[str, tuple[int, list[StepValidation]]] = {}
        self._doc_scopes: dict[str, tuple[int, list[tuple[UrlMatch, list[StepComment]]]]] = {}
        self._doc_coverages: dict[str, tuple[int, list[tuple[UrlMatch, CoverageResult]]]] = {}

    def _ensure_pattern(self):
        """Lazily build URL regex from known specs."""
        if self._url_pattern is None:
            urls = self.provider.spec_urls()
            self._url_pattern = build_url_pattern(urls)
            self._base_url_lookup = build_spec_lookup(urls)

    def _scan_doc(self, uri: str, text: str, version: int | None) -> list[UrlMatch]:
        """Scan document for spec URLs, using cache when possible."""
        self._ensure_pattern()
        version = version or 0
        cached = self._doc_urls.get(uri)
        if cached and cached[0] == version:
            return cached[1]
        matches = scan_document(text, self._url_pattern, self._base_url_lookup)
        self._doc_urls[uri] = (version, matches)
        return matches

    def _query_spec(self, spec: str, anchor: str) -> dict | None:
        """Query spec section with caching."""
        key = f"{spec}#{anchor}"
        if key in self._query_cache:
            return self._query_cache[key]
        try:
            result = self.provider.query(key)
            self._query_cache[key] = result
            return result
        except Exception:
            logger.debug("Query failed for %s", key, exc_info=True)
            return None

    def _get_algo_steps(self, anchor: str, content: str) -> list[AlgorithmStep] | None:
        """Get parsed algorithm steps with caching."""
        if anchor in self._algo_steps_cache:
            return self._algo_steps_cache[anchor]
        steps = parse_steps(content)
        if steps:
            self._algo_steps_cache[anchor] = steps
        return steps or None

    def _validate_doc(self, uri: str, text: str, version: int | None) -> list[StepValidation]:
        """Validate step comments against spec algorithms."""
        version = version or 0
        cached = self._doc_validations.get(uri)
        if cached and cached[0] == version:
            return cached[1]

        url_matches = self._scan_doc(uri, text, version)
        step_comments = scan_steps(text)
        if not url_matches or not step_comments:
            self._doc_validations[uri] = (version, [])
            self._doc_scopes[uri] = (version, [])
            return []

        scopes = build_scopes(url_matches, step_comments)
        self._doc_scopes[uri] = (version, scopes)
        validations: list[StepValidation] = []

        for url_match, steps_in_scope in scopes:
            if not steps_in_scope:
                continue

            result = self._query_spec(url_match.spec, url_match.anchor)
            if not result:
                continue

            content = result.get("content", "")
            if not content:
                continue

            algo_steps = self._get_algo_steps(url_match.anchor, content)
            if not algo_steps:
                continue

            for step_comment in steps_in_scope:
                spec_step = find_step(algo_steps, step_comment.number)
                if spec_step is None:
                    validations.append(StepValidation(
                        step=step_comment,
                        result=MatchResult.NOT_FOUND,
                        spec_text="",
                        algo_name=url_match.anchor,
                    ))
                else:
                    match_result = classify_match(
                        step_comment.text, spec_step.text,
                        threshold=self.fuzzy_threshold,
                    )
                    validations.append(StepValidation(
                        step=step_comment,
                        result=match_result,
                        spec_text=spec_step.text,
                        algo_name=url_match.anchor,
                    ))

        self._doc_validations[uri] = (version, validations)
        return validations

    def _coverage_doc(
        self, uri: str, text: str, version: int | None
    ) -> list[tuple[UrlMatch, CoverageResult]]:
        """Compute per-algorithm coverage for a document."""
        version = version or 0
        cached = self._doc_coverages.get(uri)
        if cached and cached[0] == version:
            return cached[1]

        # Ensure validations are computed (populates scopes and algo_steps caches)
        validations = self._validate_doc(uri, text, version)
        if not validations:
            self._doc_coverages[uri] = (version, [])
            return []

        # Reuse scopes computed during validation
        scopes_cached = self._doc_scopes.get(uri)
        if not scopes_cached or scopes_cached[0] != version:
            self._doc_coverages[uri] = (version, [])
            return []
        scopes = scopes_cached[1]

        results: list[tuple[UrlMatch, CoverageResult]] = []
        for url_match, steps_in_scope in scopes:
            if not steps_in_scope:
                continue

            # Reuse cached algo steps
            algo_steps = self._algo_steps_cache.get(url_match.anchor)
            if not algo_steps:
                continue

            # Filter validations to this scope by matching step lines
            scope_lines = {s.line for s in steps_in_scope}
            scope_vals = [v for v in validations if v.step.line in scope_lines]

            cov = compute_coverage(scope_vals, algo_steps, url_match.anchor)
            results.append((url_match, cov))

        self._doc_coverages[uri] = (version, results)
        return results


def _create_server(provider: SpecProvider | None = None) -> SpecLensServer:
    """Create and configure the LSP server with all handlers."""
    server = SpecLensServer(provider)
    _debounce_tasks: dict[str, asyncio.Task] = {}

    def _publish_diagnostics(uri: str, text: str, version: int | None):
        """Run step validation and publish diagnostics."""
        validations = server._validate_doc(uri, text, version)
        diagnostics: list[lsp.Diagnostic] = []
        for v in validations:
            if v.result in (MatchResult.EXACT, MatchResult.FUZZY):
                continue
            step_label = ".".join(str(n) for n in v.step.number)
            if v.result == MatchResult.NOT_FOUND:
                msg = f"Step {step_label}: not found in algorithm '{v.algo_name}'"
            else:
                msg = f"Step {step_label}: text differs from spec"
            end_line = v.step.end_line if v.step.end_line is not None else v.step.line
            diag = lsp.Diagnostic(
                range=lsp.Range(
                    start=lsp.Position(line=v.step.line, character=v.step.col_start),
                    end=lsp.Position(line=end_line, character=v.step.col_end),
                ),
                severity=lsp.DiagnosticSeverity.Warning,
                source="webspec-lens",
                message=msg,
            )
            if v.spec_text:
                diag.related_information = [
                    lsp.DiagnosticRelatedInformation(
                        location=lsp.Location(
                            uri=uri,
                            range=diag.range,
                        ),
                        message=f"Expected: {v.spec_text}",
                    )
                ]
            diagnostics.append(diag)
        server.text_document_publish_diagnostics(lsp.PublishDiagnosticsParams(
            uri=uri, diagnostics=diagnostics,
        ))

    async def _debounced_analysis(uri: str):
        """Debounced document analysis — waits before running."""
        await asyncio.sleep(_DEBOUNCE_DELAY)
        doc = server.workspace.get_text_document(uri)
        server._scan_doc(uri, doc.source, doc.version)
        _publish_diagnostics(uri, doc.source, doc.version)

    @server.feature(lsp.INITIALIZED)
    def on_initialized(params: lsp.InitializedParams):
        # Read fuzzyThreshold from initialization options
        try:
            init_opts = server.lsp._init_options
        except AttributeError:
            init_opts = None
        if isinstance(init_opts, dict):
            t = init_opts.get('fuzzyThreshold')
            if isinstance(t, (int, float)) and 0.0 <= t <= 1.0:
                server.fuzzy_threshold = float(t)

    @server.feature(lsp.TEXT_DOCUMENT_DID_OPEN)
    def did_open(params: lsp.DidOpenTextDocumentParams):
        doc = params.text_document
        server._scan_doc(doc.uri, doc.text, doc.version)
        _publish_diagnostics(doc.uri, doc.text, doc.version)

    @server.feature(lsp.TEXT_DOCUMENT_DID_CHANGE)
    async def did_change(params: lsp.DidChangeTextDocumentParams):
        uri = params.text_document.uri
        # Cancel previous debounce task for this document
        old_task = _debounce_tasks.pop(uri, None)
        if old_task is not None:
            old_task.cancel()
        # Schedule debounced analysis
        _debounce_tasks[uri] = asyncio.create_task(_debounced_analysis(uri))

    @server.feature(lsp.TEXT_DOCUMENT_DID_CLOSE)
    def did_close(params: lsp.DidCloseTextDocumentParams):
        uri = params.text_document.uri
        # Cancel any pending debounce task
        old_task = _debounce_tasks.pop(uri, None)
        if old_task is not None:
            old_task.cancel()
        server._doc_urls.pop(uri, None)
        server._doc_validations.pop(uri, None)
        server._doc_scopes.pop(uri, None)
        server._doc_coverages.pop(uri, None)
        server.text_document_publish_diagnostics(lsp.PublishDiagnosticsParams(
            uri=uri, diagnostics=[],
        ))

    @server.feature(lsp.TEXT_DOCUMENT_HOVER)
    def hover(params: lsp.HoverParams) -> lsp.Hover | None:
        uri = params.text_document.uri
        pos = params.position

        doc = server.workspace.get_text_document(uri)

        # Check spec URL hover first
        matches = server._scan_doc(uri, doc.source, doc.version)
        match = find_url_at_position(matches, pos.line, pos.character)
        if match:
            result = server._query_spec(match.spec, match.anchor)
            if result:
                markdown = build_hover_content(result)
                return lsp.Hover(
                    contents=lsp.MarkupContent(
                        kind=lsp.MarkupKind.Markdown,
                        value=markdown,
                    ),
                    range=lsp.Range(
                        start=lsp.Position(line=match.line, character=match.col_start),
                        end=lsp.Position(line=match.line, character=match.col_end),
                    ),
                )

        # Check step comment hover
        validations = server._validate_doc(uri, doc.source, doc.version)
        for v in validations:
            if v.step.line != pos.line:
                continue
            if pos.character < v.step.col_start or pos.character > v.step.col_end:
                continue

            step_label = ".".join(str(n) for n in v.step.number)
            if v.result == MatchResult.EXACT:
                md = f"**Step {step_label}** \u2014 exact match"
            elif v.result == MatchResult.FUZZY:
                md = f"**Step {step_label}** \u2014 fuzzy match"
                if v.spec_text:
                    md += f"\n\n**Spec:** {v.spec_text}"
            elif v.result == MatchResult.NOT_FOUND:
                md = f"**Step {step_label}** \u2014 not found in `{v.algo_name}`"
            else:  # MISMATCH
                md = f"**Step {step_label}** \u2014 text differs from spec"
                if v.spec_text:
                    md += f"\n\n**Expected:** {v.spec_text}"

            return lsp.Hover(
                contents=lsp.MarkupContent(
                    kind=lsp.MarkupKind.Markdown,
                    value=md,
                ),
                range=lsp.Range(
                    start=lsp.Position(line=v.step.line, character=v.step.col_start),
                    end=lsp.Position(
                        line=v.step.end_line if v.step.end_line is not None else v.step.line,
                        character=v.step.col_end,
                    ),
                ),
            )

        return None

    @server.feature(lsp.TEXT_DOCUMENT_INLAY_HINT)
    def inlay_hint(params: lsp.InlayHintParams) -> list[lsp.InlayHint] | None:
        uri = params.text_document.uri
        doc = server.workspace.get_text_document(uri)
        validations = server._validate_doc(uri, doc.source, doc.version)
        if not validations:
            return None

        hints: list[lsp.InlayHint] = []
        range_start = params.range.start.line
        range_end = params.range.end.line

        for v in validations:
            if v.step.line < range_start or v.step.line > range_end:
                continue

            step_label = ".".join(str(n) for n in v.step.number)

            if v.result in (MatchResult.EXACT, MatchResult.FUZZY):
                label = " \u2713"
                kind = lsp.InlayHintKind.Type
                if v.result == MatchResult.FUZZY and v.spec_text:
                    tooltip = lsp.MarkupContent(
                        kind=lsp.MarkupKind.Markdown,
                        value=f"**Step {step_label}** \u2014 fuzzy match\n\n"
                              f"**Spec:** {v.spec_text}",
                    )
                elif v.result == MatchResult.EXACT:
                    tooltip = lsp.MarkupContent(
                        kind=lsp.MarkupKind.Markdown,
                        value=f"**Step {step_label}** \u2014 exact match",
                    )
                else:
                    tooltip = None
            elif v.result == MatchResult.NOT_FOUND:
                label = " \u26a0"
                kind = lsp.InlayHintKind.Parameter
                tooltip = lsp.MarkupContent(
                    kind=lsp.MarkupKind.Markdown,
                    value=f"**Step {step_label}** \u2014 not found in `{v.algo_name}`",
                )
            else:  # MISMATCH
                label = " \u26a0"
                kind = lsp.InlayHintKind.Parameter
                md = f"**Step {step_label}** \u2014 text differs from spec"
                if v.spec_text:
                    md += f"\n\n**Expected:** {v.spec_text}"
                tooltip = lsp.MarkupContent(
                    kind=lsp.MarkupKind.Markdown,
                    value=md,
                )

            hints.append(lsp.InlayHint(
                position=lsp.Position(
                    line=v.step.end_line if v.step.end_line is not None else v.step.line,
                    character=v.step.col_end,
                ),
                label=label,
                kind=kind,
                tooltip=tooltip,
                padding_left=True,
            ))

        return hints if hints else None

    @server.feature(lsp.TEXT_DOCUMENT_CODE_LENS)
    def code_lens(params: lsp.CodeLensParams) -> list[lsp.CodeLens] | None:
        uri = params.text_document.uri
        doc = server.workspace.get_text_document(uri)
        coverages = server._coverage_doc(uri, doc.source, doc.version)
        if not coverages:
            return None

        lenses: list[lsp.CodeLens] = []
        for url_match, cov in coverages:
            missing_labels = [".".join(str(n) for n in s) for s in cov.missing]
            lenses.append(lsp.CodeLens(
                range=lsp.Range(
                    start=lsp.Position(line=url_match.line, character=0),
                    end=lsp.Position(line=url_match.line, character=0),
                ),
                command=lsp.Command(
                    title=cov.summary(),
                    command="webspecLens.showCoverage",
                    arguments=[
                        cov.anchor,
                        cov.total_steps,
                        missing_labels,
                    ],
                ),
            ))
        return lenses if lenses else None

    return server


def start_server(provider: SpecProvider | None = None):
    """Start the LSP server on stdio."""
    server = _create_server(provider)
    server.start_io()
