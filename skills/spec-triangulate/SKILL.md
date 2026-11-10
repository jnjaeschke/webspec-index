---
name: spec-triangulate
description: Use when a web platform behaviour may differ between the spec, web-platform-tests and the Gecko, WebKit or Chromium implementations — a WPT fails in one engine, a bug claims "the spec says X" or "other browsers do X", tests and spec seem to disagree, or you must decide whether the spec, a test or an engine is the thing that is wrong before fixing anything.
argument-hint: "<question, test path, spec anchor, or bug text>"
allowed-tools:
  - Bash(webspec-index:*)
  - Bash(searchfox-cli:*)
  - Bash(chromium-search:*)
  - Bash(gh:*)
  - Bash(curl:*)
  - Bash(python3:*)
  - Bash(git clone:*)
  - Bash(git -C:*)
  - Read
  - WebFetch
  - Agent
---

# Spec Triangulate

**Question**: $ARGUMENTS

Establish what the spec, the tests and each engine do about one behaviour, and report a sourced
verdict for every place they disagree. The output is a report, not a fix: no patches, no test edits,
no spec PRs.

**No source is the truth until checked against the others.** Tests are often written against one
engine's behaviour, engines copy each other, and the spec has bugs. Agreement between sources is a
fact to report, never evidence that the agreeing side is right.

## Requirements

`webspec-index`, `searchfox-cli` (Gecko, WebKit via `-R wubkat`, and WPT discovery),
`chromium-search`, `gh`, `curl` and `python3`, plus the sibling spec-trace skill; install
instructions are in the webspec-index README. If a tool is missing, skip that leg, say so in the
report's coverage line, and never fill the gap from memory or install the tool yourself.

**Spec repositories.** `<spec repo>` is the spec's own source repository on GitHub: `whatwg/html`,
`whatwg/dom`, `w3c/csswg-drafts`, `tc39/ecma262`, and so on. History lookups run git on its
`<checkout>`, `~/.cache/spec-triangulate/repos/<spec repo>`, which is kept across runs. A user who
already has a checkout can symlink it there
(`ln -s ~/dev/html ~/.cache/spec-triangulate/repos/whatwg/html`). Before the first history lookup on
a repository:

1. `git -C <checkout> remote -v` gives `<remote>`, the remote whose URL is
   `https://github.com/<spec repo>`; it need not be `origin`. If `<checkout>` does not exist, clone
   it: `git clone --single-branch --no-tags --no-checkout https://github.com/<spec repo> <checkout>`.
   Its remote is `origin`. Don't use `--filter=blob:none` or `--depth`: `log -S` and `log -L` read
   every historical version of the file, which a partial clone fetches one blob per round trip, and a
   shallow clone reports its oldest commit as the step's origin.
2. `git -C <checkout> fetch <remote>`.
3. `<ref>` is `<remote>/<default branch>`, the branch from
   `gh api repos/<spec repo> --jq .default_branch`. Pass it to every `log` and `grep`. A linked
   checkout is the user's working copy, so its `HEAD` and working tree can be on any branch: never
   read them, and never run `checkout`, `switch`, `reset` or `pull` in it.

Engine issue trackers (Bugzilla, issues.chromium.org, bugs.webkit.org) are never searched: they hold
each engine's opinion. Open a tracker entry only when a code comment, a commit message or an
engine-exported WPT change cites it. When the session has a dedicated tool for a source — `bmo-to-md`
or an MCP server for Bugzilla, the MDN MCP server — use it for those reads instead of `curl` /
WebFetch.

## Execution

Work linearly by default. When the question spans more than one algorithm family or more than three
tests, give each engine's code investigation its own subagent. Its brief holds the scenario, the spec
steps and anchors from Phase 1, the specific questions for that engine, and for Chromium the literal
commit hash from Phase 3; it returns each claim with a line-level permalink. Open the link of every
subagent claim a verdict rests on yourself.

## Phase 0: Frame

State the behaviour and the **scenario**: the preconditions that decide branches. Then list every
claim the input makes — the bug's description of correct behaviour, "engine X does Y", a previous
patch's or agent's conclusion. Each is checked in the phases below and ends up confirmed, refuted
or open. None of them is evidence.

## Phase 1: Spec

1. Trace the behaviour with Phases 0–2 and the hard rules of the spec-trace skill
   (`../spec-trace/SKILL.md`). Its verdicts, Render section and `--gecko` mode don't apply here.
   Record every normative step the scenario passes through, with its step number.
2. **Sweep the non-normative text.** Notes, examples and introductory prose often state the intended
   outcome outright. For every algorithm, concept and IDL member the question turns on:
   `webspec-index refs '<SPEC#anchor>' -d incoming --kind note -l 50`, the same with `--kind prose`,
   and `webspec-index search '<key phrase>' -s <SPEC>` for the observable outcome in words ("is
   rejected", "is not fired", "must not"). Also read the author-facing summary of each IDL member the
   scenario calls, where the spec has one (WHATWG's "For web developers" boxes): it often states the
   outcome in prose. Record what the sweep found, or that it found nothing.
3. **Pending changes.** Search the spec's repository for open PRs touching these algorithms
   (`gh search prs --repo <spec repo> '<algorithm name>' --state open`). For each, read the
   proposed text with `webspec-index query '<SPEC#anchor>' --pr N` (WHATWG and TC39) and judge the
   question under both texts.
4. **History.** For each deciding step, find when it got its current form and why:
   `gh search prs --repo <spec repo> '<algorithm name>' --merged` and
   `gh search issues --repo <spec repo> '<algorithm name>'`, then
   `gh pr view N --repo <spec repo> --comments` for the rationale. Search misses PRs that changed a
   step without naming it; to date a step's text, use the spec repository's checkout (see
   Requirements). The source file holds the step with its markup (`Let <var>url</var> be a new`), so
   find it first: `git -C <checkout> grep -n '<step phrase>' <ref> -- <source file>` gives its
   source text and lines. Then run
   `git -C <checkout> log -S '<distinctive source text>' <ref> -- <source file>`. `-S` matches the
   exact string, so it finds nothing, or stops at an editorial commit, once the text was rewrapped or
   re-indented. Then follow the step's lines instead:
   `git -C <checkout> log -L <start>,<end>:<source file> --no-patch <ref>`. This is slow on
   whatwg/html's `source`, so use it only after `-S`.
5. **Pre-GitHub rationale.** Commits from before a spec moved to GitHub cite W3C Bugzilla instead
   of issues (`Fixing http://www.w3.org/Bugs/Public/show_bug.cgi?id=N`); read the cited bug. When a
   bug or commit points at chat discussion, read the WHATWG logs for the commit's date and the days
   before it: `https://krijnhoetmer.github.io/irc-logs/whatwg/<YYYYMMDD>.html` (2006–2016),
   `https://matrixlogs.bakkot.com/irc-whatwg/<YYYY-MM-DD>` (2016–2021),
   `https://matrixlogs.bakkot.com/WHATWG/<YYYY-MM-DD>` (2021 on).

## Phase 2: Tests

1. **Find the tests**: `searchfox-cli --spec-refs '<spec section URL>'` lists tests citing a section;
   also search the test directory by API name. Searchfox serves Gecko's vendored copy, which can lag
   upstream, so use it for discovery only. Read and link each test upstream at one commit: the
   wpt.fyi run's `full_revision_hash` from step 3 (`gh api -H 'Accept: application/vnd.github.raw'
   'repos/web-platform-tests/wpt/contents/<test path>?ref=<sha>'`).
2. **One row per assertion.** For each relevant assertion or expected value, record the spec step it
   exercises and whether it agrees with Phase 1. A test can be right in one assertion and wrong in
   the next.
3. **Results today.** Get the latest runs with
   `curl -s 'https://wpt.fyi/api/runs?label=master&label=experimental&product=chrome&product=firefox&product=safari&max-count=1'`
   (repeat each parameter; comma-separated values are rejected), then
   `curl -s 'https://wpt.fyi/api/search?run_ids=<a>,<b>,<c>&q=<test path>'` for per-test status.
   Per-assertion status and failure messages: take the run's `results_url`, drop
   `-summary_v2.json.gz`, append `/<test path without leading slash>`, and fetch that. A test's
   top-level `status` (`OK`, shown as `"O"` in search results) is the harness status and says nothing
   about passing; pass and fail are each subtest's `status`, and `passes`/`total` in search results.
   Expectation files say the same per engine: Gecko `testing/web-platform/meta/<test path>.ini`,
   Chromium `third_party/blink/web_tests/external/wpt/<test path minus extension>-expected.txt`
   (plus `TestExpectations` and platform or virtual baselines), WebKit
   `LayoutTests/imported/w3c/web-platform-tests/<test path minus extension>-expected.txt`. A missing
   expectation file means the engine expects the test to pass.
4. **Origin.** `gh api 'repos/web-platform-tests/wpt/commits?path=<test path>'` gives each change's
   author and date (searchfox blame only shows Gecko's sync commits). If the oldest commit is a move
   or rename, continue on its old path:
   `gh api repos/web-platform-tests/wpt/commits/<sha> --jq '.files[] | select(.filename=="<test path>") | .previous_filename'`.
   Engine-exported changes name their source: Chromium commits carry `Reviewed-on:` and `Bug:` lines,
   Gecko ones say `[Gecko Bug N]`, WebKit ones say `WebKit export of https://bugs.webkit.org/…`.
   Compare dates with Phase 1 history: a test written before the spec step changed may be stale; a
   test exported from one engine may encode that engine's behaviour.

## Phase 3: Engines

For each of Gecko (`searchfox-cli`), WebKit (`searchfox-cli -R wubkat`) and Chromium
(`chromium-search`):

1. **Trace the code; don't skim it.** For each deciding spec step, find the engine function that
   implements it and establish the scenario's path to it with the call graph:
   - Gecko and WebKit: `searchfox-cli [-R wubkat] --calls-to '<ns::Class::method>' --depth N`,
     `--calls-from`, `--calls-between '<A>,<B>'`, always with the fully qualified name
     (`mozilla::dom::…`, `WebCore::…`). Output with no entries means the name did not resolve, not
     that nothing calls it: confirm with `searchfox-cli --id` or `-q` before concluding anything.
     Read bodies with plain `searchfox-cli --define '<ns::Class::method>'`; adding `--permalink`
     makes it print only a URL.
   - Chromium: first resolve the commit once,
     `curl -s --max-time 20 'https://chromium.googlesource.com/chromium/src/+/refs/heads/main?format=JSON' | python3 -c 'import json,sys; print(json.loads(sys.stdin.read()[5:])["commit"])'`,
     and paste that literal hash into every later command, link and subagent brief (shell variables
     don't survive between commands). Callers: `chromium-search 'usage:<name>'`. Bodies:
     `chromium-search cat <path> --ref <hash> -n`. `chromium-search` reports any failure, a rate
     limit included, as `File not found`; retry, or read the file from gitiles:
     `curl -s 'https://chromium.googlesource.com/chromium/src/+/<hash>/<path>?format=TEXT' | python3 -c 'import base64,sys; sys.stdout.write(base64.b64decode(sys.stdin.read()).decode())'`.

   Read the body of every function on the path, including the branches the scenario takes inside
   it. A claim about what engine code does is stated as the chain of lines you read, each with its
   link. Behaviour you infer without reading the code is not a claim; it goes under open questions.
2. **Build the call-site map** (a required report section). For each algorithm the question turns
   on, and for each piece of state whose change triggers it:
   - Spec side: its callers (`webspec-index refs '<SPEC#anchor>' -d incoming --kind step -l 50`)
     and writers (`webspec-index state '<SPEC#field>'`). Note which writers go through an algorithm
     with side effects and which assign directly.
   - Engine side: start at the function implementing the algorithm, and at every thin wrapper
     around it (a setter or helper whose job is to run it). Run `--calls-to` (Chromium: `usage:`) on
     each, then on each caller it returns, and keep going up. A caller is a site once the code
     around the call is a numbered step of the spec algorithm that caller implements; record it and
     pair it with that step.
   - Pair each engine site with the spec step it implements. An engine site with no spec step, a
     spec step with no engine site, or an engine site that runs side effects where the spec
     assigns directly, is a mismatch row. A mismatch that changes what the scenario observes is also
     a finding with its own verdict.
3. **Cited issues are leads.** A spec issue or tracker entry cited by a code comment, a commit
   message or an engine-exported WPT change: check its current state and how it was resolved. A
   workaround for an issue resolved differently is stale.
4. **Shipping status**, if relevant:
   `curl -s https://bcd.developer.mozilla.org/bcd/api/v0/current/<bcd key>.json`. It says whether
   an engine ships the feature, never whether its behaviour conforms.

An engine's code comment explaining its behaviour is that engine's claim, checked like any other.

## Phase 4: Cross-checks

- **Universal claims need an enumeration.** Before writing "contradicts everything else", "nothing
  else needs to change" or "the only place", run the sweep that would show otherwise and cite it.
- **Masking.** When a proposed change removes code, list every path that code ran on. Behaviour that
  code was suppressing may surface on those paths. Check tests in the same directory that exercise
  them.
- **Surroundings.** Read the steps around each quoted spec step, the setup and neighbouring asserts
  of each test, and the call context of each engine line before concluding what it means.

## Phase 5: Coherence

This is reasoning, reported as such and never as a fact. Ask:

- What would a web developer expect? Read the MDN page for the API and its examples.
- Do sibling entry points (for the Navigation API: `navigate()`, `reload()`, `traverseTo()`,
  `back()`, `forward()`) resolve the same edge case the same way?
- How do analogous, older APIs behave?

An unexplained inconsistency is worth reporting. When the spec settles a question explicitly, the
verdict follows the spec, and a poor design goes under design observations. Circular justification
is a finding too: a spec change argued from tests that were written against one engine.

## Verdicts

One per finding:

| Verdict | When |
|---|---|
| **ENGINE BUG** (name engines) | An explicit normative step on the scenario's path; the engine diverges from it. |
| **TEST BUG** (stale / engine-shaped) | An explicit normative step; the assertion contradicts it. *Stale*: the spec changed after the test. *Engine-shaped*: the test matches an engine and never matched the spec. |
| **NOBODY FOLLOWS THE SPEC** | An explicit normative step, and every engine diverges from it, whether or not they agree with each other or the tests. Used instead of ENGINE BUG when every engine diverges. |
| **SPEC PENDING** (PR #) | An open spec PR changes the answer. Heading form: `SPEC PENDING (#N): <verdict under the merged text> / <verdict under the PR>`. |
| **SPEC GAP** | The spec is silent, ambiguous or UA-defined on the point. |
| **UNRESOLVED** | The evidence doesn't decide. Say what would. |
| **NO DIVERGENCE** | Spec, tests and every checked engine agree. Name any engine not checked. |

When a test assertion and an engine are wrong in different ways, they are separate findings, each
with its own verdict.

A verdict other than UNRESOLVED needs, for the algorithms its finding turns on, the Phase 1
notes-and-prose sweep, the Phase 3 call-site map for the engines the finding names, and every
subagent claim it rests on opened by you at its line-level link. If any of these is missing for a
finding, that finding's verdict is UNRESOLVED and the coverage line names what is missing. A skipped
engine leg affects only findings that make claims about that engine.

ENGINE BUG and TEST BUG follow the spec text however many engines agree with the other side. When
several engines follow a test that contradicts the spec, keep the verdict, say so in the finding,
and list "change the spec" under ways forward.

## Report

Draft the report with these parts, in order:

```markdown
**Question:** <behaviour>
**Scenario:** <branch-deciding preconditions>
**Coverage:** <legs run; legs skipped and why>

### <Finding title> — **<VERDICT>**

<Two to four sentences: what diverges and why.>

| Source | Says | Link |
|---|---|---|
| Spec, normative | <step, quoted> | <anchor, step N> |
| Spec, notes and prose | <quote, or "none: <sweep commands run>"> | |
| Spec history | <PR/issue and what it changed> | |
| Test `<file>`: `<assertion>` | <expects> | <permalink> |
| Gecko / WebKit / Chromium | <does> | <permalink> |
| wpt.fyi | <pass/fail per engine> | <url> |

**Ways forward:** for each option, what it changes, its pros, and its cons.

### Call-site map

One table per algorithm or state from Phase 3 step 2. Every engine site that is an extra site, a
missing site, or runs side effects where the spec assigns is a row, wrapper callers included. Sites
that match go on one line under the table: "Matching: <function> (<spec step>), …".

| Engine | Site | Spec step it implements | Mismatch |
|---|---|---|---|
| <engine> | <function, permalink> | <anchor step N, or "none"> | <extra site / missing site / side effects where spec assigns> |

### Design observations
<Reasoning, not verdicts.>

### Open questions
<What could not be established, and what would establish it.>
```

The input's claims from Phase 0 each appear as confirmed (in a finding), refuted (in a finding), or
open (under open questions).

When the draft is complete and every link in it is verified, edit it as described in the last
section. The edited report is your reply.

## Links

Every factual sentence has a link you opened in this session.

| Source | Link form |
|---|---|
| Spec | `webspec-index query … --links full` anchors; quote the step number in the link text |
| Gecko, WebKit | `searchfox-cli [-R wubkat] --get-file <path> --lines <N> --permalink`, which gives `searchfox.org/<repo>/rev/<hash>/<path>#<N>`; a `/source/` URL is not a permalink |
| Chromium | `https://source.chromium.org/chromium/chromium/src/+/<hash>:<path>;l=<N>` with the hash from Phase 3, lines confirmed with `chromium-search cat <path> --ref <hash> -n` — never `+/main:` |
| Upstream WPT | `https://github.com/web-platform-tests/wpt/blob/<sha>/<path>#L<N>`, `<sha>` the wpt.fyi run's `full_revision_hash` from Phase 2 |
| wpt.fyi | the per-test JSON URL from Phase 2 |
| Issues, PRs, commits | their GitHub URLs |
| W3C Bugzilla, WHATWG chat logs | the bug URL; the log page for the day plus the message's line anchor (`#l-N` on krijnhoetmer.github.io, `#LN` on matrixlogs.bakkot.com) |

## Red flags

| Thought | Reality |
|---|---|
| "The tests and two engines agree, so the spec must be the outlier." | Agreement shows a shared choice, often one engine's tests copied by others. The spec text decides the verdict; agreement goes under ways forward. |
| "This step contradicts the rest of the spec." | Only after the Phase 1 notes-and-prose sweep. Quote what it found. |
| "Engine X's comment explains why it has to work this way." | That comment is engine X's claim. Check it against the spec. |
| "Making this engine do what the others do fixes the test." | Matching other engines is not a spec argument. |
| "This engine calls the algorithm in a few extra places; I'll work around that." | Extra call sites are a finding. Report them; don't build around them. |
| "The test is correct." | Only per assertion, each with its own row. |

## Editing the report

This pass changes how the report reads, never what it says.

**Shape of the reply:**

1. **TL;DR**: one or two sentences giving each finding's verdict and the action it points to.
2. The analysis, in the draft's section order.

**Rewrite:**

- Prose paragraphs become bullet lists, one fact per bullet. Each link stays attached to the fact
  it supports.
- Table cells keep one fact plus its link. A spec quote may be shortened to its deciding clause,
  with `…` marking every elision.
- A finding's evidence table keeps the rows its verdict rests on, and always the "Spec, notes and
  prose" row.
- Where two sections say the same thing, keep it where it decides something.

**Keep unchanged:** every verdict, every link, every call-site map row, each input claim's status,
and the coverage line.
