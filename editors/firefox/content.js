"use strict";

const SPEC_URL_RE =
  /https?:\/\/(?:[\w-]+\.spec\.whatwg\.org|drafts\.(?:csswg|fxtf|css-houdini)\.org|www\.w3\.org\/TR\/[\w-]+|w3c\.github\.io\/[\w-]+|wicg\.github\.io\/[\w-]+|webaudio\.github\.io\/[\w-]+|tc39\.es\/ecma\d+|webassembly\.github\.io\/[\w-]+|www\.rfc-editor\.org\/rfc\/[\w.-]+|datatracker\.ietf\.org\/doc\/html\/[\w.-]+)(?:\/[^\s"'<>]*)?#[\w%.:(){}[\]-]+/g;

// ── Surrounding code context ───────────────────────────────────────────────

const COMMENT_LINE_RE = /^\s*(?:\/\/|\/\*|\*|#|;)\s*(.+)/;

// Matches section references anywhere in text:
//   §5.3.4   section 5.3   sec. 5   5.3.4 (multi-part only)
const SEC_REF_RE = /§\s*(\d+(?:\.\d+)*)|(?:section|sec\.)\s+(\d+(?:\.\d+)+)|(?<!\d)(\d{1,3}(?:\.\d{1,3})+)(?!\d)/gi;

// Walk the DOM to extract text lines surrounding `el`.
// Works for Phabricator (<tr> per line in a diff table) and
// Searchfox (<div> per line in the source view).
function extractSurroundingLines(el) {
  let lineEl = el.closest('tr');
  if (!lineEl) {
    let cur = el.parentElement;
    while (cur && cur.parentElement) {
      const p = cur.parentElement;
      if (p.id === 'file' || p.classList.contains('source') ||
          p.classList.contains('source-listing') ||
          Array.from(p.children).length > 5) {
        lineEl = cur;
        break;
      }
      cur = p;
    }
  }
  if (!lineEl || !lineEl.parentElement) return [];

  const siblings = Array.from(lineEl.parentElement.children);
  const idx = siblings.indexOf(lineEl);
  if (idx === -1) return [];

  return siblings.map((sib, i) => ({
    text: sib.textContent.replace(/\t/g, '    '),
    isAnchor: sib === lineEl,
    delta: i - idx,
  }));
}

// Scan all comment lines in the window, extract unique section reference
// numbers (§5.3.4, section 5.4, bare 5.3.4). Returns an array of strings.
function extractNearbySectionRefs(domLines) {
  const anchorIdx = domLines.findIndex(l => l.isAnchor);
  if (anchorIdx === -1) return [];

  const refs = new Set();
  for (let i = 0; i < domLines.length; i++) {
    if (i === anchorIdx) continue;
    const { text } = domLines[i];
    if (!text.trim()) continue;
    const cm = COMMENT_LINE_RE.exec(text);
    if (!cm) continue;
    SEC_REF_RE.lastIndex = 0;
    let m;
    while ((m = SEC_REF_RE.exec(cm[1])) !== null) {
      const num = m[1] || m[2] || m[3];
      if (num) refs.add(num);
    }
  }
  return [...refs];
}

// ── Cache & history ────────────────────────────────────────────────────────

const resultCache = new Map();
let popupHistory = [];  // [{query, result, contextResult, nearbySectionRefs, highlightSteps}]
let currentQuery = null;

async function fetchSpec(query) {
  if (resultCache.has(query)) return resultCache.get(query);
  const result = await browser.runtime.sendMessage({ type: "query", url: query });
  resultCache.set(query, result);
  return result;
}

// ── Popup ──────────────────────────────────────────────────────────────────

const POPUP_ID = "webspec-lens-popup";

function getOrCreatePopup() {
  let popup = document.getElementById(POPUP_ID);
  if (popup) return popup;

  const style = document.createElement("style");
  style.textContent = `
    #${POPUP_ID} {
      position: fixed; z-index: 2147483647;
      width: 480px; max-height: 520px; overflow-y: auto;
      background: Canvas; color: CanvasText;
      border: 1px solid ButtonBorder; border-radius: 6px;
      padding: 12px 14px; font-size: 13px; line-height: 1.5;
      font-family: system-ui, sans-serif;
      box-shadow: 0 4px 16px rgba(0,0,0,.25);
      display: none;
      color-scheme: light dark;
    }
    #${POPUP_ID} .ws-nav { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
    #${POPUP_ID} .ws-back { background: ButtonFace; border: 1px solid ButtonBorder; color: ButtonText; border-radius: 4px; padding: 2px 8px; cursor: pointer; font-size: 12px; }
    #${POPUP_ID} .ws-back:hover { opacity: .8; }
    #${POPUP_ID} .ws-breadcrumb { font-size: 11px; color: GrayText; }
    #${POPUP_ID} .ws-breadcrumb a { color: LinkText; cursor: pointer; text-decoration: none; }
    #${POPUP_ID} .ws-breadcrumb a:hover { text-decoration: underline; }
    #${POPUP_ID} h3 { margin: 0 0 2px; font-size: 14px; }
    #${POPUP_ID} .ws-meta { font-size: 11px; color: GrayText; margin-bottom: 8px; }
    #${POPUP_ID} .ws-meta a { color: LinkText; text-decoration: none; }
    #${POPUP_ID} .ws-meta a:hover { text-decoration: underline; }
    #${POPUP_ID} .ws-content p { margin: 4px 0; }
    #${POPUP_ID} .ws-content code { background: ButtonFace; padding: 1px 4px; border-radius: 3px; font-size: 12px; font-family: monospace; }
    #${POPUP_ID} .ws-content strong { font-weight: 600; }
    #${POPUP_ID} .ws-content em { font-style: italic; }
    #${POPUP_ID} .ws-content h4 { margin: 8px 0 2px; font-size: 13px; }
    #${POPUP_ID} .ws-content ol { list-style: none !important; counter-reset: ws-step; margin: 4px 0; padding: 0; }
    #${POPUP_ID} .ws-content ol li { counter-increment: ws-step; padding-left: 28px; position: relative; margin: 3px 0; }
    #${POPUP_ID} .ws-content ol li::before { content: counter(ws-step) "."; position: absolute; left: 0; color: GrayText; min-width: 24px; }
    #${POPUP_ID} .ws-content ol ol { counter-reset: ws-step; margin-left: 0; }
    #${POPUP_ID} .ws-content ul { list-style: none !important; margin: 4px 0; padding: 0; }
    #${POPUP_ID} .ws-content ul li { padding-left: 14px; position: relative; margin: 2px 0; }
    #${POPUP_ID} .ws-content ul li::before { content: "•"; position: absolute; left: 0; color: GrayText; }
    #${POPUP_ID} .ws-content a { color: LinkText; text-decoration: none; cursor: pointer; }
    #${POPUP_ID} .ws-content a:hover { text-decoration: underline; }
    #${POPUP_ID} .ws-content li.ws-highlighted { outline: 2px solid Highlight; outline-offset: 1px; border-radius: 2px; }
    #${POPUP_ID} .ws-refs { margin-top: 10px; border-top: 1px solid ButtonBorder; padding-top: 8px; }
    #${POPUP_ID} .ws-refs-list { display: flex; flex-wrap: wrap; gap: 4px; }
    #${POPUP_ID} .ws-ref-chip { background: ButtonFace; border: 1px solid ButtonBorder; border-radius: 4px; padding: 2px 7px; font-size: 11px; color: LinkText; cursor: pointer; white-space: nowrap; }
    #${POPUP_ID} .ws-ref-chip:hover { opacity: .8; }
    #${POPUP_ID} .ws-loading { color: GrayText; font-style: italic; }
    #${POPUP_ID} .ws-error { color: red; }
    #${POPUP_ID} .ws-open { float: right; font-size: 11px; color: GrayText; text-decoration: none; margin-left: 8px; }
    #${POPUP_ID} .ws-open:hover { color: LinkText; }
    #${POPUP_ID} details.ws-context { margin-top: 10px; border-top: 1px solid ButtonBorder; padding-top: 8px; }
    #${POPUP_ID} details.ws-context summary { font-size: 11px; color: GrayText; cursor: pointer; user-select: none; }
    #${POPUP_ID} .ws-sec-ref { background: ButtonFace; border: 1px solid ButtonBorder; border-radius: 4px; padding: 2px 7px; font-size: 11px; cursor: pointer; white-space: nowrap; font-family: monospace; }
    #${POPUP_ID} .ws-sec-ref:hover { opacity: .8; }
    #${POPUP_ID} .ws-search-results { margin-top: 8px; }
    #${POPUP_ID} .ws-search-item { padding: 4px 0; border-bottom: 1px solid ButtonBorder; cursor: pointer; }
    #${POPUP_ID} .ws-search-item:hover { color: LinkText; }
    #${POPUP_ID} .ws-search-item .ws-si-title { font-size: 13px; }
    #${POPUP_ID} .ws-search-item .ws-si-anchor { font-size: 11px; color: #6c7086; font-family: monospace; }
  `;
  document.head.appendChild(style);

  popup = document.createElement("div");
  popup.id = POPUP_ID;
  document.body.appendChild(popup);
  return popup;
}

function positionPopup(popup, anchorEl) {
  const rect = anchorEl.getBoundingClientRect();
  const vw = window.innerWidth;
  const vh = window.innerHeight;

  const spaceBelow = vh - rect.bottom - 8;
  const spaceAbove = rect.top - 8;
  if (spaceBelow >= 80 || spaceBelow >= spaceAbove) {
    popup.style.top = (rect.bottom + 6) + "px";
    popup.style.bottom = "auto";
  } else {
    popup.style.bottom = (vh - rect.top + 6) + "px";
    popup.style.top = "auto";
  }
  const left = Math.max(8, Math.min(rect.left, vw - 496));
  popup.style.left = left + "px";
}

function escHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// Render inline markdown: bold, italic, code spans, links.
// Spec URLs become popup-navigable <a class="ws-spec-link"> elements.
// Relative #anchor links resolve against `spec`.
function renderInline(text, spec) {
  const codeChunks = [];
  // Extract code spans. If a code span's entire content is a markdown link
  // (htmd artefact: `[[[slot]]](url)` → should be <a><code>[[slot]]</code></a>)
  // tag it for special rendering.
  let s = text.replace(/`([^`\n]+)`/g, (_, code) => {
    const lm = code.match(/^\[(.+)\]\(([^)]+)\)$/);
    codeChunks.push(lm ? { linkText: lm[1], url: lm[2] } : code);
    return `\x00CODE${codeChunks.length - 1}\x00`;
  });

  // Stash markdown escape sequences (e.g. \[ \] from [[slot]] in htmd output)
  // before HTML-escaping so the link regex sees plain [ ] chars.
  const escChunks = [];
  s = s.replace(/\\([^\s])/g, (_, ch) => {
    escChunks.push(ch);
    return `\x00ESC${escChunks.length - 1}\x00`;
  });

  s = s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

  s = s.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_, linkText, url) => {
    const isSpec = SPEC_URL_RE.test(url);
    SPEC_URL_RE.lastIndex = 0;
    if (isSpec) {
      return `<a class="ws-spec-link" data-query="${escHtml(url)}">${linkText}</a>`;
    }
    if (url.startsWith("#") && spec) {
      const query = `${spec}${url}`;
      return `<a class="ws-spec-link" data-query="${escHtml(query)}">${linkText}</a>`;
    }
    return `<a href="${escHtml(url)}" target="_blank" rel="noopener">${linkText}</a>`;
  });

  s = s.replace(/\*\*([^*\n]+)\*\*/g, "<strong>$1</strong>");
  s = s.replace(/\*([^*\n]+)\*/g, "<em>$1</em>");

  s = s.replace(/\x00ESC(\d+)\x00/g, (_, i) => escHtml(escChunks[+i]));

  s = s.replace(/\x00CODE(\d+)\x00/g, (_, i) => {
    const chunk = codeChunks[+i];
    if (typeof chunk === 'object') {
      const codeHtml = `<code>${escHtml(chunk.linkText)}</code>`;
      const isSpec = SPEC_URL_RE.test(chunk.url); SPEC_URL_RE.lastIndex = 0;
      if (isSpec) return `<a class="ws-spec-link" data-query="${escHtml(chunk.url)}">${codeHtml}</a>`;
      return `<a href="${escHtml(chunk.url)}" target="_blank" rel="noopener">${codeHtml}</a>`;
    }
    return `<code>${escHtml(chunk)}</code>`;
  }
  );

  return s;
}

// htmd produces "loose" lists: blank lines between every item.
// Merge all consecutive list blocks (top-level or indented) into one
// flat line array before parsing, so nesting is reconstructed correctly.
function mergeListBlocks(blocks) {
  const out = [];
  let listLines = null;

  const isListBlock = (b) => /^\s*\d+\.\s/.test(b.trim());

  for (const block of blocks) {
    if (!block.trim()) continue;
    if (isListBlock(block)) {
      if (!listLines) listLines = [];
      for (const l of block.split('\n')) listLines.push(l);
      listLines.push(''); // blank separator preserved for parseItems to skip
    } else {
      if (listLines) { out.push({ type: 'list', lines: listLines }); listLines = null; }
      out.push({ type: 'text', content: block });
    }
  }
  if (listLines) out.push({ type: 'list', lines: listLines });
  return out;
}

// Parse a flat array of lines (may contain blanks) into a nested item tree.
// Handles `1. ` and `1.  ` (one or more spaces after dot).
function parseListItems(lines, baseIndent) {
  const items = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const m = line.match(/^(\s*)\d+\.\s+(.*)/);
    if (!m) { i++; continue; }
    const indent = m[1].length;
    if (indent < baseIndent) break;
    if (indent > baseIndent) { i++; continue; }
    let text = m[2].trim();
    i++;
    // Collect sub-lines: all lines (including blanks) until we hit a
    // same-or-lower-indent numbered item.
    const subLines = [];
    while (i < lines.length) {
      const sub = lines[i];
      if (!sub.trim()) { subLines.push(sub); i++; continue; }
      const sm = sub.match(/^(\s*)\d+\.\s/);
      if (sm && sm[1].length <= indent) break;
      subLines.push(sub);
      i++;
    }
    const childItems = subLines.length
      ? parseListItems(subLines, Math.min(...subLines.filter(l => /^\s*\d+\.\s/.test(l)).map(l => l.match(/^(\s*)/)[1].length).filter(n => !isNaN(n)), Infinity) || indent + 1)
      : [];
    items.push({ text, children: childItems });
  }
  return items;
}

function renderListItems(items, spec) {
  if (!items.length) return '';
  let html = '<ol>';
  for (const item of items) {
    html += `<li>${renderInline(item.text, spec)}`;
    if (item.children.length) html += renderListItems(item.children, spec);
    html += '</li>';
  }
  return html + '</ol>';
}

// Render spec content markdown to HTML.
// Handles htmd "loose" list format (blank lines between every item).
function renderMarkdown(md, spec) {
  if (!md) return "";

  const rawBlocks = md.split(/\n{2,}/);
  const blocks = mergeListBlocks(rawBlocks);

  return blocks.map(b => {
    if (b.type === 'list') {
      const firstList = b.lines.find(l => /^\s*\d+\.\s/.test(l));
      if (!firstList) return '';
      const baseIndent = firstList.match(/^(\s*)/)[1].length;
      const items = parseListItems(b.lines, baseIndent);
      return renderListItems(items, spec);
    }
    const block = b.content.trim();
    if (!block) return '';
    if (/^#{1,3}\s/.test(block)) {
      return block.replace(/^#{1,3}\s+(.+)$/m, (_, t) => `<h4>${renderInline(t, spec)}</h4>`);
    }
    const lines = block.split('\n').filter(l => l.trim());
    if (/^[•\-]\s/.test(lines[0])) {
      return '<ul>' + lines.map(l => `<li>${renderInline(l.replace(/^[•\-]\s+/, ''), spec)}</li>`).join('') + '</ul>';
    }
    return '<p>' + lines.map(l => renderInline(l.trim(), spec)).join(' ') + '</p>';
  }).join('');
}

// Highlight a nested list step. `steps` is an array of 1-based integers.
// Walks ol > li[steps[0]-1] > ol > li[steps[1]-1] > ... and highlights the deepest.
function highlightStep(container, steps) {
  if (!steps || !steps.length) return;

  let ol = container.querySelector("ol");
  if (!ol) return;

  let target = null;
  for (let depth = 0; depth < steps.length; depth++) {
    const idx = steps[depth] - 1;
    if (idx < 0 || idx >= ol.children.length) return;
    target = ol.children[idx];
    if (depth < steps.length - 1) {
      ol = target.querySelector("ol");
      if (!ol) return;
    }
  }

  if (!target) return;

  target.classList.add("ws-highlighted");

  // Scroll the popup div itself, not the page.
  const popup = target.closest("#" + POPUP_ID);
  if (popup) {
    const pr = popup.getBoundingClientRect();
    const tr = target.getBoundingClientRect();
    if (tr.bottom > pr.bottom) popup.scrollTop += tr.bottom - pr.bottom + 8;
    else if (tr.top < pr.top) popup.scrollTop -= pr.top - tr.top + 8;
  }
}

function renderPopup(popup, result, anchorEl, contextResult, nearbySectionRefs, highlightSteps) {
  const spec = result.spec || "";
  const anchor = result.anchor || "";
  const title = result.title || anchor;
  const specAnchor = spec && anchor ? `${spec}#${anchor}` : "";
  const openUrl = result._sourceUrl || "";

  let html = "";

  // Back button
  if (popupHistory.length > 0) {
    const prev = popupHistory[popupHistory.length - 1];
    const prevLabel = prev.result.title || prev.result.anchor || "back";
    html += `<div class="ws-nav">
      <button class="ws-back" id="ws-back-btn">← ${escHtml(prevLabel)}</button>
    </div>`;
  }

  // Parent breadcrumb — show parent only, title is in h3
  if (result.parent) {
    const pTitle = result.parent.title || result.parent.anchor;
    html += `<div class="ws-breadcrumb">
      <a class="ws-spec-link" data-query="${escHtml(spec + "#" + result.parent.anchor)}">${escHtml(pTitle)}</a>
      <span> ›</span>
    </div>`;
  }

  // Title + meta
  html += `<h3>${escHtml(title)}`;
  if (openUrl) {
    html += `<a class="ws-open" href="${escHtml(openUrl)}" target="_blank" rel="noopener" title="Open in spec">↗</a>`;
  }
  html += `</h3>`;

  if (specAnchor) {
    const typeLabel = result.section_type && result.section_type.toLowerCase() !== "heading"
      ? escHtml(result.section_type) + " · " : "";
    html += `<div class="ws-meta">${typeLabel}<code>${escHtml(specAnchor)}</code></div>`;
  }

  // Own content
  const contentHtml = renderMarkdown(result.content || "", spec);
  if (contentHtml) {
    html += `<div class="ws-content">${contentHtml}</div>`;
  }

  // Parent context — shown when own content is absent, or always as collapsible
  if (contextResult) {
    const ctxSpec = contextResult.spec || spec;
    const ctxTitle = contextResult.title || contextResult.anchor || "";
    const ctxHtml = renderMarkdown(contextResult.content || "", ctxSpec);
    if (ctxHtml) {
      if (!contentHtml) {
        // No own content: show parent inline
        html += `<div class="ws-content">${ctxHtml}</div>`;
      } else {
        // Has own content: show parent collapsed
        html += `<details class="ws-context">
          <summary>Context: ${escHtml(ctxTitle)}</summary>
          <div class="ws-content">${ctxHtml}</div>
        </details>`;
      }
    }
  } else if (!contentHtml) {
    html += `<p style="color:#6c7086;margin:4px 0">No content available.</p>`;
  }

  // Outgoing refs + nearby section refs from code comments as chips
  const hasRefs = (result.outgoing_refs && result.outgoing_refs.length > 0);
  const hasSectionRefs = (nearbySectionRefs && nearbySectionRefs.length > 0);
  if (hasRefs || hasSectionRefs) {
    html += `<div class="ws-refs"><div class="ws-refs-list">`;
    if (hasRefs) {
      for (const ref of result.outgoing_refs) {
        const q = `${ref.spec}#${ref.anchor}`;
        html += `<span class="ws-ref-chip" data-query="${escHtml(q)}">${escHtml(q)}</span>`;
      }
    }
    if (hasSectionRefs) {
      for (const num of nearbySectionRefs) {
        html += `<span class="ws-sec-ref" data-sec-spec="${escHtml(spec)}" data-sec-num="${escHtml(num)}">§${escHtml(num)}</span>`;
      }
    }
    html += `</div></div>`;
  }

  popup.innerHTML = html;
  popup.style.display = "block";

  if (anchorEl) positionPopup(popup, anchorEl);

  // Highlight step if requested
  if (highlightSteps) {
    highlightStep(popup.querySelector(".ws-content"), highlightSteps);
  }

  // Wire up back button
  const backBtn = popup.querySelector("#ws-back-btn");
  if (backBtn) {
    backBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      const prev = popupHistory.pop();
      if (prev) {
        currentQuery = prev.query;
        renderPopup(popup, prev.result, null, prev.contextResult, prev.nearbySectionRefs, prev.highlightSteps);
      }
    });
  }

  // Wire up all spec navigation links and ref chips
  popup.querySelectorAll("[data-query]").forEach((el) => {
    el.addEventListener("click", (e) => {
      e.preventDefault();
      e.stopPropagation();
      navigatePopup(popup, el.dataset.query, result, contextResult, nearbySectionRefs, highlightSteps);
    });
  });

  // Wire up section-reference chips in dev notes
  popup.querySelectorAll(".ws-sec-ref").forEach((el) => {
    el.addEventListener("click", (e) => {
      e.stopPropagation();
      const secSpec = el.dataset.secSpec;
      const num = el.dataset.secNum;
      showSearchResults(popup, secSpec, num, result, contextResult, nearbySectionRefs, highlightSteps);
    });
  });
}

function navigatePopup(popup, query, fromResult, fromContext, fromSectionRefs, fromHighlightSteps) {
  popupHistory.push({ query: currentQuery, result: fromResult, contextResult: fromContext, nearbySectionRefs: fromSectionRefs, highlightSteps: fromHighlightSteps });
  currentQuery = query;

  popup.innerHTML = '<span class="ws-loading">Loading…</span>';
  loadWithContext(query).then(({ result, contextResult }) => {
    if (popup.style.display === "none") return;
    if (result.ok) renderPopup(popup, result, null, contextResult, null, null);
    else popup.innerHTML = `<span class="ws-error">⚠ ${escHtml(result.error || "error")}</span>`;
  }).catch((err) => {
    popup.innerHTML = `<span class="ws-error">⚠ ${escHtml(err.message)}</span>`;
  });
}

// Fetch a spec section, then fetch its parent as context if own content is absent.
async function loadWithContext(query) {
  const result = await fetchSpec(query);
  if (!result.ok) return { result, contextResult: null };

  if (!result.content && result.parent && result.spec) {
    const parentQuery = `${result.spec}#${result.parent.anchor}`;
    try {
      const ctx = await fetchSpec(parentQuery);
      return { result, contextResult: ctx.ok ? ctx : null };
    } catch {
      // parent fetch failed — show what we have
    }
  }
  return { result, contextResult: null };
}

// ── Show / hide ────────────────────────────────────────────────────────────

let hideTimer = null;
let anchorForPopup = null;
let anchorInitialY = null;

function trackAnchor(anchor) {
  anchorForPopup = anchor;
  anchorInitialY = anchor.getBoundingClientRect().top;
  let lastTop = anchorInitialY;
  (function check() {
    const popup = document.getElementById(POPUP_ID);
    if (!popup || popup.style.display === "none") return;
    const rect = anchor.getBoundingClientRect();
    if (rect.bottom < 0 || rect.top > window.innerHeight) {
      popup.style.display = "none";
      popupHistory = [];
      currentQuery = null;
      return;
    }
    if (Math.abs(rect.top - lastTop) > 1) {
      lastTop = rect.top;
      positionPopup(popup, anchor);
    }
    requestAnimationFrame(check);
  })();
}

async function searchSpec(spec, query) {
  return browser.runtime.sendMessage({ type: "search", spec, query });
}

function showSearchResults(popup, spec, query, fromResult, fromContext, fromSectionRefs, fromHighlightSteps) {
  popupHistory.push({ query: currentQuery, result: fromResult, contextResult: fromContext, nearbySectionRefs: fromSectionRefs, highlightSteps: fromHighlightSteps });
  currentQuery = null;

  popup.innerHTML = `<span class="ws-loading">Searching ${escHtml(spec)} for "${escHtml(query)}"…</span>`;

  searchSpec(spec, query).then((resp) => {
    if (popup.style.display === "none") return;
    if (!resp.ok || !resp.search_results || resp.search_results.length === 0) {
      popup.innerHTML = `<span class="ws-error">No results for "${escHtml(query)}" in ${escHtml(spec)}</span>`;
      return;
    }
    let html = '';
    if (popupHistory.length > 0) {
      const prev = popupHistory[popupHistory.length - 1];
      html += `<div class="ws-nav"><button class="ws-back" id="ws-back-btn">← ${escHtml(prev.result?.title || prev.result?.anchor || "back")}</button></div>`;
    }
    html += `<div class="ws-dev-label">Results for "${escHtml(query)}" in ${escHtml(spec)}</div>`;
    html += '<div class="ws-search-results">';
    for (const item of resp.search_results) {
      const q = `${item.spec}#${item.anchor}`;
      html += `<div class="ws-search-item" data-query="${escHtml(q)}">
        <div class="ws-si-title">${escHtml(item.title || item.anchor)}</div>
        <div class="ws-si-anchor">${escHtml(q)}</div>
      </div>`;
    }
    html += '</div>';
    popup.innerHTML = html;

    const backBtn = popup.querySelector("#ws-back-btn");
    if (backBtn) {
      backBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        const prev = popupHistory.pop();
        if (prev) {
          currentQuery = prev.query;
          renderPopup(popup, prev.result, null, prev.contextResult, prev.nearbySectionRefs, prev.highlightSteps);
        }
      });
    }
    popup.querySelectorAll(".ws-search-item[data-query]").forEach((el) => {
      el.addEventListener("click", (e) => {
        e.stopPropagation();
        navigatePopup(popup, el.dataset.query, fromResult, fromContext, fromSectionRefs, fromHighlightSteps);
      });
    });
  }).catch((err) => {
    popup.innerHTML = `<span class="ws-error">⚠ ${escHtml(err.message)}</span>`;
  });
}

// Find the nearest .webspec-lens-link span that precedes `el` in document order
// within the closest relevant container.
function findNearestSpecLink(el) {
  const container = el.closest('tbody, #file, .differential-diff, .source-listing') || document.body;
  const links = Array.from(container.querySelectorAll('.webspec-lens-link'));
  if (!links.length) return null;

  let preceding = null;
  let following = null;
  for (const link of links) {
    const pos = link.compareDocumentPosition(el);
    if (pos & Node.DOCUMENT_POSITION_FOLLOWING) {
      preceding = link; // link precedes el — keep updating, last = nearest
    } else if ((pos & Node.DOCUMENT_POSITION_PRECEDING) && !following) {
      following = link; // first link that follows el
    }
  }
  return preceding || following;
}

// Returns true if the closest relevant container of node.parentElement
// contains at least one .webspec-lens-link element.
function hasNearbySpecLink(node) {
  const parent = node.parentElement;
  if (!parent) return false;
  const container = parent.closest('tbody, #file, .differential-diff, .source-listing') || document.body;
  return container.querySelector('.webspec-lens-link') !== null;
}

// Build a map of section-number → heading from a flat list of headings.
// Bikeshed specs have unnumbered preamble sections (abstract, sotd, toc) —
// skip anchors whose titles look non-normative before the first normative h2.
const NON_NORMATIVE = new Set(['abstract', 'sotd', 'toc', 'contents', 'status']);
function buildSectionMap(headings) {
  const counters = [0, 0, 0, 0, 0, 0, 0]; // indices 0-6, use 2-6
  const map = {};
  let seenNormative = false;

  for (const h of headings) {
    const d = h.depth;
    if (d < 2 || d > 6) continue;
    const isNonNormative = NON_NORMATIVE.has(h.anchor.toLowerCase());
    if (!seenNormative && isNonNormative) continue;
    seenNormative = true;

    counters[d]++;
    for (let i = d + 1; i <= 6; i++) counters[i] = 0;
    const num = counters.slice(2, d + 1).join('.');
    map[num] = h;
  }
  return map;
}

async function showForSecRef(span, numStr, secType) {
  clearTimeout(hideTimer);
  popupHistory = [];

  const popup = getOrCreatePopup();
  popup.innerHTML = '<span class="ws-loading">Loading…</span>';
  popup.style.display = "block";
  positionPopup(popup, span);
  trackAnchor(span);

  const linkEl = findNearestSpecLink(span);
  if (!linkEl) {
    popup.innerHTML = `<span class="ws-error">No nearby spec URL found</span>`;
    return;
  }

  const specUrl = linkEl.dataset.specUrl;
  let specResult = resultCache.get(specUrl);
  if (!specResult) specResult = await fetchSpec(specUrl).catch(() => null);
  if (!specResult?.ok) {
    popup.innerHTML = `<span class="ws-error">⚠ Could not resolve spec</span>`;
    return;
  }
  const specName = specResult.spec;

  const listResp = await browser.runtime.sendMessage({ type: "list", spec: specName }).catch(() => null);
  if (!listResp?.ok || !listResp.headings?.length) {
    popup.innerHTML = `<span class="ws-error">⚠ Could not list sections for ${escHtml(specName)}</span>`;
    return;
  }

  const sectionMap = buildSectionMap(listResp.headings);

  // §5.3.4: try 5.3.4 → not found, try 5.3 → found (algorithm), remaining [4] = step
  const parts = numStr.split('.');
  let anchor = null;
  let remainingSteps = null;
  let matchedSectionNum = null;
  for (let len = parts.length; len >= 1; len--) {
    const sectionNum = parts.slice(0, len).join('.');
    const hit = sectionMap[sectionNum];
    if (hit) {
      anchor = `${specName}#${hit.anchor}`;
      matchedSectionNum = sectionNum;
      if (len < parts.length) remainingSteps = parts.slice(len).map(Number);
      break;
    }
  }

  if (!anchor) {
    popup.innerHTML = `<span class="ws-error">§${escHtml(numStr)} not found in ${escHtml(specName)}</span>`;
    return;
  }

  const { result, contextResult } = await loadWithContext(anchor)
    .catch(() => ({ result: { ok: false, error: 'fetch failed' }, contextResult: null }));
  if (!result.ok) {
    popup.innerHTML = `<span class="ws-error">⚠ ${escHtml(result.error || 'error')}</span>`;
    return;
  }

  // If we found a section by trimming the suffix (e.g. §5.3.4 → section 5.3,
  // step [4]), always highlight — the user explicitly referenced a sub-step.
  const highlightSteps = remainingSteps?.length ? remainingSteps : null;

  const displayResult = { ...result, _sourceUrl: anchor };
  if (matchedSectionNum) {
    displayResult.title = `§${matchedSectionNum}${result.title ? ' — ' + result.title : ''}`;
  }
  result._sourceUrl = anchor;
  currentQuery = anchor;
  renderPopup(popup, displayResult, span, contextResult, null, highlightSteps);
}

function annotateCommentRefs(node) {
  const text = node.nodeValue;
  if (!text) return;

  SEC_REF_RE.lastIndex = 0;
  const matches = [];
  let m;
  while ((m = SEC_REF_RE.exec(text)) !== null) {
    const num = m[1] || m[2] || m[3];
    if (!num) continue;
    const secType = (m[1] || m[2]) ? 'section' : 'step';
    matches.push({ start: m.index, end: m.index + m[0].length, num, secType, raw: m[0] });
  }
  if (!matches.length) return;

  const frag = document.createDocumentFragment();
  let pos = 0;
  for (const { start, end, num, secType, raw } of matches) {
    if (start > pos) frag.appendChild(document.createTextNode(text.slice(pos, start)));

    const span = document.createElement("span");
    span.className = "webspec-lens-secref";
    span.textContent = raw;
    span.dataset.secNum = num;
    span.dataset.secType = secType;
    span.style.cssText = "color:#89dceb;text-decoration:underline dotted;cursor:pointer";

    span.addEventListener("mouseenter", () => showForSecRef(span, num, secType));
    span.addEventListener("mouseleave", scheduleHide);

    frag.appendChild(span);
    pos = end;
  }
  if (pos < text.length) frag.appendChild(document.createTextNode(text.slice(pos)));
  node.parentNode.replaceChild(frag, node);
}

function showForLink(span, url) {
  clearTimeout(hideTimer);
  popupHistory = [];
  currentQuery = url;
  // Extract surrounding code comments before the async fetch so we have them
  // immediately when the result comes back.
  const domLines = extractSurroundingLines(span);
  const nearbySectionRefs = extractNearbySectionRefs(domLines);

  const popup = getOrCreatePopup();
  popup.innerHTML = '<span class="ws-loading">Loading…</span>';
  popup.style.display = "block";
  positionPopup(popup, span);
  trackAnchor(span); // must come after display:block so RAF sees visible popup

  loadWithContext(url).then(({ result, contextResult }) => {
    if (popup.style.display === "none") return;
    if (result.ok) {
      result._sourceUrl = url;
      renderPopup(popup, result, span, contextResult, nearbySectionRefs.length ? nearbySectionRefs : null, null);
    } else {
      popup.innerHTML = `<span class="ws-error">⚠ ${escHtml(result.error || "error")}</span>`;
    }
  }).catch((err) => {
    popup.innerHTML = `<span class="ws-error">⚠ ${escHtml(err.message)}</span>`;
  });
}

function scheduleHide() {
  hideTimer = setTimeout(() => {
    const popup = document.getElementById(POPUP_ID);
    if (popup) popup.style.display = "none";
    popupHistory = [];
    currentQuery = null;
  }, 300);
}

// ── DOM annotation ─────────────────────────────────────────────────────────

function annotateTextNode(node) {
  const text = node.nodeValue;
  if (!text) return;

  SPEC_URL_RE.lastIndex = 0;
  const matches = [];
  let m;
  while ((m = SPEC_URL_RE.exec(text)) !== null) {
    matches.push({ start: m.index, end: m.index + m[0].length, url: m[0] });
  }
  if (matches.length === 0) return;

  const frag = document.createDocumentFragment();
  let pos = 0;
  for (const { start, end, url } of matches) {
    if (start > pos) frag.appendChild(document.createTextNode(text.slice(pos, start)));

    const a = document.createElement("a");
    a.textContent = text.slice(start, end);
    a.className = "webspec-lens-link";
    a.href = url;
    a.target = "_blank";
    a.rel = "noopener";
    a.style.cssText = "color:#89b4fa;text-decoration:underline dotted;cursor:pointer";
    a.dataset.specUrl = url;

    a.addEventListener("mouseenter", () => showForLink(a, url));
    a.addEventListener("mouseleave", scheduleHide);

    frag.appendChild(a);
    pos = end;
  }
  if (pos < text.length) frag.appendChild(document.createTextNode(text.slice(pos)));
  node.parentNode.replaceChild(frag, node);
}

function scanRoot(root) {
  // Never annotate inside our own popup.
  if (root.id === POPUP_ID || root.closest?.("#" + POPUP_ID)) return;

  // Pass 1: annotate spec URLs
  const walker1 = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const p = node.parentElement;
      if (!p) return NodeFilter.FILTER_REJECT;
      if (p.closest("#" + POPUP_ID)) return NodeFilter.FILTER_REJECT;
      const tag = p.tagName;
      if (tag === "SCRIPT" || tag === "STYLE" || tag === "NOSCRIPT") return NodeFilter.FILTER_REJECT;
      if (p.classList && p.classList.contains("webspec-lens-link")) return NodeFilter.FILTER_REJECT;
      return NodeFilter.FILTER_ACCEPT;
    },
  });

  const urlNodes = [];
  let n;
  while ((n = walker1.nextNode())) {
    SPEC_URL_RE.lastIndex = 0;
    if (SPEC_URL_RE.test(n.nodeValue)) urlNodes.push(n);
  }
  for (const node of urlNodes) {
    if (node.parentNode) annotateTextNode(node);
  }

  // Pass 2: annotate section references near spec links
  const walker2 = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const p = node.parentElement;
      if (!p) return NodeFilter.FILTER_REJECT;
      if (p.closest("#" + POPUP_ID)) return NodeFilter.FILTER_REJECT;
      const tag = p.tagName;
      if (tag === "SCRIPT" || tag === "STYLE" || tag === "NOSCRIPT") return NodeFilter.FILTER_REJECT;
      if (p.classList && (p.classList.contains("webspec-lens-link") || p.classList.contains("webspec-lens-secref"))) return NodeFilter.FILTER_REJECT;
      return NodeFilter.FILTER_ACCEPT;
    },
  });

  const secRefNodes = [];
  while ((n = walker2.nextNode())) {
    SEC_REF_RE.lastIndex = 0;
    if (SEC_REF_RE.test(n.nodeValue) && hasNearbySpecLink(n)) secRefNodes.push(n);
  }
  for (const node of secRefNodes) {
    if (node.parentNode) annotateCommentRefs(node);
  }
}

// ── Init ───────────────────────────────────────────────────────────────────

function init() {
  scanRoot(document.body);

  new MutationObserver((mutations) => {
    for (const mut of mutations) {
      for (const node of mut.addedNodes) {
        if (node.nodeType === Node.ELEMENT_NODE) scanRoot(node);
      }
    }
  }).observe(document.body, { childList: true, subtree: true });

  // Keep popup open when mouse enters it, hide when it leaves.
  document.addEventListener("mouseover", (e) => {
    if (e.target.closest("#" + POPUP_ID)) clearTimeout(hideTimer);
  });
  document.addEventListener("mouseout", (e) => {
    if (e.target.closest("#" + POPUP_ID)) scheduleHide();
  });

  // Hide immediately on scroll (the anchor element moves, popup stays fixed).
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", init);
} else {
  init();
}
