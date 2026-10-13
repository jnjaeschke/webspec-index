"""Type stubs for the compiled webspec-index extension module."""

from typing import Any, Optional
from .effects import (
    EffectsRequest, ExplainEffectsRequest, RecomputeEffectsRequest,
    EffectSummaryResult, ExplainEffectsResult, RecomputeEffectsResult,
)

__version__: str

def get_effect_summary(request: EffectsRequest) -> EffectSummaryResult: ...
def explain_effects(request: ExplainEffectsRequest) -> ExplainEffectsResult: ...
def recompute_effects(request: RecomputeEffectsRequest) -> RecomputeEffectsResult: ...

class WebspecError(Exception):
    """Raised when a webspec-index operation fails."""

# ── Result types ─────────────────────────────────────────────────────
#
# All result classes are frozen (read-only attributes) and provide
# ``to_dict()``, ``to_json()`` and ``__repr__``.

class NavEntry:
    anchor: str
    title: Optional[str]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class RefEntry:
    spec: str
    anchor: str
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class Navigation:
    parent: Optional[NavEntry]
    prev: Optional[NavEntry]
    next: Optional[NavEntry]
    children: list[NavEntry]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class QueryResult:
    spec: str
    sha: str
    anchor: str
    title: Optional[str]
    section_type: str
    content: Optional[str]
    navigation: Navigation
    outgoing_refs: list[RefEntry]
    incoming_refs: list[RefEntry]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class ExistsResult:
    exists: bool
    spec: str
    anchor: str
    section_type: Optional[str]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class AnchorEntry:
    spec: str
    anchor: str
    title: Optional[str]
    section_type: str
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class AnchorsResult:
    pattern: str
    results: list[AnchorEntry]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class SearchEntry:
    spec: str
    anchor: str
    title: Optional[str]
    section_type: str
    snippet: str
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class SearchResult:
    query: str
    results: list[SearchEntry]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class ListEntry:
    anchor: str
    title: Optional[str]
    depth: int
    parent: Optional[str]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class SpecUrlEntry:
    spec: str
    base_url: str
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class RefsMatch:
    spec: str
    anchor: str
    title: Optional[str]
    section_type: str
    resolution: str
    outgoing: Optional[list[RefEntry]]
    incoming: Optional[list[RefEntry]]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class RefsResult:
    query: str
    direction: str
    matches: list[RefsMatch]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class IdlEntry:
    spec: str
    anchor: str
    kind: str
    name: str
    owner: Optional[str]
    canonical_name: str
    title: Optional[str]
    idl_text: Optional[str]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class IdlResult:
    query: str
    matches: list[IdlEntry]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class GraphRoot:
    spec: str
    anchor: str
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class GraphNode:
    id: str
    spec: str
    anchor: str
    title: Optional[str]
    section_type: Optional[str]
    filter_role: Optional[str]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class GraphEdge:
    from_: str  # serializes to JSON key "from"
    to: str
    kind: str
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class GraphResult:
    root: GraphRoot
    direction: str
    max_depth: int
    max_nodes: int
    truncated: bool
    nodes: list[GraphNode]
    edges: list[GraphEdge]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class PrDiffSummary:
    added: int
    removed: int
    modified: int
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class PrDiffEntry:
    anchor: str
    title: Optional[str]
    change_type: str
    old_content: Optional[str]
    new_content: Optional[str]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class PrDiffResult:
    spec: str
    pr_number: int
    head_sha: str
    merge_base_sha: str
    summary: PrDiffSummary
    changes: list[PrDiffEntry]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class UpdateEntry:
    spec: str
    updated: bool
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class CoverageSummary:
    total: int
    implemented: int
    missing: list[list[int]]
    warnings: int
    reordered: int
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class StepValidation:
    line: int
    col: int
    step: list[int]
    comment_text: str
    result: str
    spec_text: str
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class ScopeAnalysis:
    spec: str
    anchor: str
    url: str
    line: int
    col: int
    validations: list[StepValidation]
    coverage: Optional[CoverageSummary]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

class FileAnalysis:
    file: str
    scopes: list[ScopeAnalysis]
    def to_json(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...

# ── Functions ────────────────────────────────────────────────────────

def query_view(
    spec_anchor: str,
    involving: str | list[str] | None = ...,
    feeding: str | None = ...,
    steps: str | list[str] | None = ...,
    depth: int | None = ...,
    links: str = ...,
    no_notes: bool = ...,
) -> dict:
    """Return a sliced view of one algorithm as a dict.

    The dict contains all ``query`` fields plus ``content`` (the slice-rendered
    markdown, with link and note transformations applied) and ``slice`` (a
    ``SliceResult`` object describing kept steps, omitted runs, matched
    variables, and status counts).

    *involving*, *steps*: a variable/step name or a list of them.
    *feeding*: ``"STEP"`` or ``"STEP:VAR,…"`` — earlier steps that define the
    variables used by *STEP*.
    *links*: ``"short"`` (default), ``"full"``, or ``"none"``.

    Raises ``WebspecError`` for selector errors or when the section has no
    slice index.  Raises ``ValueError`` for an invalid *links* value.
    """
    ...

def state(
    selector: str,
    include_inits: bool = True,
    unclassified: bool = False,
    limit: Optional[int] = None,
    calls: bool = False,
    callers: bool = False,
    opaque: bool = False,
) -> Any:
    """Query possible writes (state) for a selector.

    *selector* is one of ``TYPE``, ``TYPE.field``, or ``SPEC#anchor``.
    *calls*, *callers*, and *opaque* request algorithm call, caller, and opaque-statement details.
    Returns a dict matching the ``StateResponse`` JSON schema.
    Raises ``WebspecError`` when the spec has no state model or the selector
    is not found; for ambiguous selectors the candidates are appended one per
    line to the error message.
    """
    ...

def state_coverage(spec: str) -> Any:
    """Return state coverage counters for an indexed specification.

    Returns a dict matching the ``StateCoverageResult`` JSON schema.
    Raises ``WebspecError`` when the spec has no indexed state model.
    """
    ...

def query(
    spec_anchor: str, pr: Optional[int] = ..., force_update: bool = ...
) -> QueryResult: ...
def exists(
    spec_anchor: str, pr: Optional[int] = ..., force_update: bool = ...
) -> ExistsResult: ...
def search(
    query: str,
    spec: Optional[str] = ...,
    limit: int = ...,
    pr: Optional[int] = ...,
    force_update: bool = ...,
) -> SearchResult: ...
def anchors(
    pattern: str,
    spec: Optional[str] = ...,
    limit: int = ...,
    pr: Optional[int] = ...,
    force_update: bool = ...,
) -> AnchorsResult: ...
def list_headings(
    spec: str, pr: Optional[int] = ..., force_update: bool = ...
) -> list[ListEntry]: ...
def refs(
    target: str,
    direction: str = ...,
    limit: int = ...,
    pr: Optional[int] = ...,
    force_update: bool = ...,
) -> RefsResult: ...
def graph(
    spec_anchor: str,
    direction: str = ...,
    max_depth: int = ...,
    max_nodes: int = ...,
    include: Optional[list[str]] = ...,
    exclude: Optional[list[str]] = ...,
    same_spec_only: bool = ...,
) -> GraphResult: ...
def idl(
    query: str,
    spec: Optional[str] = ...,
    limit: int = ...,
    pr: Optional[int] = ...,
    force_update: bool = ...,
) -> IdlResult: ...
def pr_diff(spec: str, pr: int, force_update: bool = ...) -> PrDiffResult: ...
def update(spec: Optional[str] = ..., force: bool = ...) -> list[UpdateEntry]: ...
def clear_pr(
    spec: Optional[str] = ..., pr: Optional[int] = ..., all: bool = ...
) -> Any: ...
def clear_db() -> str: ...
def specs() -> list[SpecUrlEntry]: ...
def parse_anchor(input: str) -> tuple[str, str, Optional[str]]: ...
def analyze(
    path: str, recursive: bool = ..., threshold: float = ...
) -> list[FileAnalysis]: ...
