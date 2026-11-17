// A very small code editor: a textarea over a highlighted copy of its text, with line numbers.
// Lua only. Tab indents, Enter keeps the indentation. Grows with its content, no inner scroll.
"use strict";

const LUA_KEYWORDS = new Set(["and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
  "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while"]);
const LUA_GLOBALS = new Set(["ui", "http", "json", "blob", "log", "print", "config", "string", "table", "math", "os", "utf8",
  "assert", "error", "pairs", "ipairs", "tostring", "tonumber", "type", "select", "pcall", "setmetatable", "getmetatable"]);

// Long comments, comments, long strings, strings, numbers, names; anything else passes through.
const LUA_TOKENS = /(--\[(=*)\[[\s\S]*?(?:\]\2\]|$))|(--[^\n]*)|(\[(=*)\[[\s\S]*?(?:\]\5\]|$))|("(?:\\[\s\S]|[^"\\\n])*"?|'(?:\\[\s\S]|[^'\\\n])*'?)|(\b0[xX][\da-fA-F]+\b|\b\d+(?:\.\d*)?(?:[eE][+-]?\d+)?\b)|([A-Za-z_]\w*)/g;

function highlightLua(src) {
  let out = "", last = 0;
  for (const m of src.matchAll(LUA_TOKENS)) {
    out += esc(src.slice(last, m.index));
    const t = esc(m[0]);
    if (m[1] || m[3]) out += `<span class="tk-com">${t}</span>`;
    else if (m[4] || m[6]) out += `<span class="tk-str">${t}</span>`;
    else if (m[7]) out += `<span class="tk-num">${t}</span>`;
    else if (LUA_KEYWORDS.has(m[0])) out += `<span class="tk-kw">${t}</span>`;
    else if (LUA_GLOBALS.has(m[0])) out += `<span class="tk-glob">${t}</span>`;
    else out += t;
    last = m.index + m[0].length;
  }
  return out + esc(src.slice(last));
}

/// Mounts an editor in `host`. Returns { get value, set value, set readOnly, focus }.
function codeEditor(host, { value = "", readOnly = false, minLines = 4, label = "Code", onInput } = {}) {
  host.classList.add("code");
  host.innerHTML = `<div class="code-gutter" aria-hidden="true"></div>
    <div class="code-scroll"><div class="code-area">
      <pre class="code-hl" aria-hidden="true"></pre>
      <textarea spellcheck="false" autocapitalize="off" autocomplete="off" wrap="off" aria-label="${esc(label)}"></textarea>
    </div></div>`;
  const ta = host.querySelector("textarea"), pre = host.querySelector(".code-hl"), gutter = host.querySelector(".code-gutter");

  const sync = () => {
    const text = ta.value;
    const lines = text.split("\n");
    // A trailing space keeps the last empty line's height in the <pre>.
    pre.innerHTML = highlightLua(text) + " ";
    const n = Math.max(lines.length, minLines);
    gutter.textContent = Array.from({ length: n }, (_, i) => i + 1).join("\n");
    ta.style.height = `calc(${n} * var(--code-lh) + 2 * var(--code-pad))`;
    const widest = lines.reduce((m, l) => Math.max(m, l.length), 0);
    host.querySelector(".code-area").style.width = `max(100%, calc(${widest + 2}ch + 2 * var(--code-pad)))`;
  };

  const edit = (insert, from = ta.selectionStart, to = ta.selectionEnd) => {
    ta.setRangeText(insert, from, to, "end");
    sync();
    onInput && onInput(ta.value);
  };

  ta.addEventListener("input", () => { sync(); onInput && onInput(ta.value); });
  ta.addEventListener("keydown", e => {
    if (ta.readOnly) return;
    if (e.key === "Tab" && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      if (e.shiftKey) {
        // Remove up to two spaces at the start of the line.
        const start = ta.value.lastIndexOf("\n", ta.selectionStart - 1) + 1;
        const n = ta.value.slice(start, start + 2).match(/^ */)[0].length;
        if (n) { const keep = ta.selectionStart - n; edit("", start, start + n); ta.setSelectionRange(Math.max(start, keep), Math.max(start, keep)); }
      } else {
        edit("  ");
      }
    } else if (e.key === "Enter" && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      const start = ta.value.lastIndexOf("\n", ta.selectionStart - 1) + 1;
      const indent = ta.value.slice(start).match(/^[ \t]*/)[0];
      edit("\n" + indent);
    }
  });

  const api = {
    get value() { return ta.value; },
    set value(v) { ta.value = v ?? ""; sync(); },
    set readOnly(r) { ta.readOnly = r; host.classList.toggle("readonly", r); },
    focus() { ta.focus(); },
  };
  api.value = value;
  api.readOnly = readOnly;
  return api;
}
