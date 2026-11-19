// What both workspaces of the web UI (Screens: index.html, Devices: devices.html) are built on:
// talking to the server, the page frame (top bar, pages by #/hash, polling), and small widgets
// (messages, questions, relative times). Adapted from bps.
"use strict";

const API = "/api/v1";
const POLL_MS = 5000;
const $ = id => document.getElementById(id);

/// What every page may read: the server's devices and status as last fetched.
const App = {
  devices: [],
  status: null,         // GET /status
  online: true,         // false while the server cannot be reached
  pages: [],            // this workspace's pages; the first is the default
  onRoute: null,        // (route) => void: a page was opened
  onTick: null,         // async (route) => void: time to refresh what is on screen
};

// ---- the server ----

class ApiError extends Error {
  constructor(status, reason) { super(reason); this.status = status; }
}
/// Calls the control API; the reply's JSON (null for 204). Errors carry the server's reason.
async function api(path, opts) {
  let r;
  try { r = await fetch(API + path, opts); } catch (e) { throw new ApiError(0, "the server cannot be reached"); }
  if (r.status === 401) {
    // Signed out, or the session ended: sign in and come back here.
    location.href = "/login?next=" + encodeURIComponent(location.pathname + location.hash);
    throw new ApiError(401, "sign in");
  }
  if (!r.ok) {
    const text = await r.text();
    let reason = text;
    try { reason = JSON.parse(text).error || text; } catch (e) { /* plain text */ }
    throw new ApiError(r.status, reason || `${r.status} ${r.statusText}`);
  }
  // An empty answer (204, or 201 with no body) is null.
  const text = await r.text();
  return text ? JSON.parse(text) : null;
}
function apiSend(method, path, body) {
  return api(path, { method, headers: { "content-type": "application/json" }, body: body === undefined ? undefined : JSON.stringify(body) });
}

function deviceName(d) { return d.label || d.name || d.mac; }

// ---- formatting ----

function esc(s) {
  return String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}
function fmtFull(ms) { return new Date(ms).toLocaleString(); }
function fmtAgo(ms) {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 10) return "just now";
  if (s < 60) return `${s} s ago`;
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return `${Math.floor(s / 86400)} d ago`;
}

// ---- small DOM helpers ----

let pressedAt = null;  // what a pointer is pressing, until it lets go
document.addEventListener("pointerdown", e => { pressedAt = e.target; }, true);
for (const k of ["pointerup", "pointercancel"]) document.addEventListener(k, () => { pressedAt = null; }, true);
/// Sets an element's HTML only when it differs, so what the user is pointing at, has open or is
/// typing into is left alone between refreshes. True if it changed.
/// Held back while a mouse button or finger is down inside the element: rebuilding it between
/// press and release would lose the click.
function setHtml(el, html) {
  if (typeof el === "string") el = $(el);
  if (!el || el.__html === html) return false;
  if (pressedAt && el.contains(pressedAt)) return false;
  el.innerHTML = html;
  el.__html = html;
  return true;
}
function setText(el, text) {
  if (typeof el === "string") el = $(el);
  if (el && el.textContent !== text) el.textContent = text;
}
/// "5 min ago", kept current by a ticker without redrawing what it sits in.
function ago(ms) {
  return ms ? `<time class="ago" data-ms="${ms}" title="${esc(fmtFull(ms))}">${esc(fmtAgo(ms))}</time>` : "never";
}
setInterval(() => {
  for (const t of document.querySelectorAll("time.ago")) setText(t, fmtAgo(Number(t.dataset.ms)));
}, 1000);

// ---- messages and questions ----

/// A message in the corner that goes away by itself (errors stay until dismissed).
function toast(text, kind) {
  let box = document.querySelector(".toasts");
  if (!box) { box = document.createElement("div"); box.className = "toasts"; box.setAttribute("role", "status"); document.body.append(box); }
  const el = document.createElement("div");
  el.className = "toast " + (kind || "");
  el.innerHTML = `<span class="grow">${esc(text)}</span><button class="small" type="button">Dismiss</button>`;
  el.querySelector("button").onclick = () => el.remove();
  box.append(el);
  if (kind !== "bad") setTimeout(() => el.remove(), 5000);
}
/// Asks before something that cannot be taken back. Resolves true if confirmed.
function confirmDialog({ title, body, action, danger }) {
  return new Promise(resolve => {
    const d = document.createElement("dialog");
    d.innerHTML = `<h2>${esc(title)}</h2><div class="body">${body}</div><div class="actions">` +
      `<button type="button" value="no">Cancel</button><button type="button" value="yes" class="${danger ? "danger" : "primary"}">${esc(action)}</button></div>`;
    document.body.append(d);
    let answer = false;
    for (const b of d.querySelectorAll("button")) b.onclick = () => { answer = b.value === "yes"; d.close(); };
    d.addEventListener("close", () => { d.remove(); resolve(answer); });
    d.showModal();
    d.querySelector('button[value="no"]').focus();
  });
}

// ---- pages: #/page/more?param=value ----

function route() {
  const [path, query] = location.hash.replace(/^#\/?/, "").split("?");
  const parts = path.split("/").filter(Boolean).map(decodeURIComponent);
  const page = App.pages.includes(parts[0]) ? parts[0] : App.pages[0];
  return { page, parts: parts.slice(1), params: new URLSearchParams(query || "") };
}
/// "#/page/more?a=b" from its pieces; empty values are left out.
function href(page, parts, params) {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params || {})) if (v !== "" && v != null && v !== false) q.set(k, v);
  const path = [page, ...(parts || [])].filter(p => p != null && p !== "").map(encodeURIComponent).join("/");
  return "#/" + path + (q.toString() ? "?" + q : "");
}
function showPage() {
  const r = route();
  for (const p of App.pages) { const el = $("page-" + p); if (el) el.hidden = p !== r.page; }
  for (const a of document.querySelectorAll(".topbar nav a")) {
    const current = a.dataset.page === r.page;
    a.classList.toggle("current", current);
    if (current) a.setAttribute("aria-current", "page"); else a.removeAttribute("aria-current");
  }
  const name = document.querySelector(`.topbar nav a[data-page="${r.page}"]`)?.textContent || "";
  document.title = `${name} - tephra`;
  if (App.onRoute) App.onRoute(r);
  refresh();
}

// ---- polling ----

let refreshing = false, refreshAgain = false;
async function refresh() {
  if (refreshing) { refreshAgain = true; return; }
  refreshing = true;
  try {
    const [devices, status] = await Promise.all([api("/devices"), api("/status")]);
    App.devices = devices;
    App.status = status;
    setOnline(true);
    renderStatus();
    if (App.onTick) await App.onTick(route());
  } catch (e) {
    if (e instanceof ApiError && e.status === 0) setOnline(false);
    else { console.error(e); showProblem(e.message); }
  } finally {
    refreshing = false;
    if (refreshAgain) { refreshAgain = false; refresh(); }
  }
}
function setOnline(online) {
  App.online = online;
  const b = $("offline");
  if (b) b.hidden = online;
  if (online) showProblem("");
  else renderStatus();
}
function showProblem(text) {
  const b = $("problem");
  if (!b) return;
  b.hidden = !text;
  setText(b.querySelector("span"), text);
}
/// The top bar's right-hand side, in one line: how many connected devices are checking in
/// (seen in the last two hours). Placeholders don't count.
function renderStatus() {
  const el = $("status");
  if (!el) return;
  if (!App.online) { setHtml(el, `<span class="dot bad"></span> server not reachable`); return; }
  const real = App.devices.filter(d => !d.placeholder);
  const recent = real.filter(d => d.last_seen_ms && Date.now() - d.last_seen_ms < 2 * 3600e3).length;
  const total = real.length;
  const dot = !total ? "off" : recent === total ? "ok" : "bad";
  const text = !total ? "no device connected" : `${recent} of ${total} device${total === 1 ? "" : "s"} checking in`;
  setHtml(el, `<a href="/devices"><span class="dot ${dot}"></span> ${esc(text)}</a>`);
}

/// Who is signed in, with a way out; nothing while the lock is off and nobody is.
async function showUser() {
  const el = $("foot-user");
  if (!el) return;
  try {
    const me = await api("/auth/me");
    if (me.via === "login") {
      el.innerHTML = `Signed in as ${esc(me.name)} <button type="button" class="link" id="sign-out">Sign out</button>`;
      $("sign-out").onclick = async () => {
        await apiSend("POST", "/auth/logout").catch(() => {});
        location.href = me.locked ? "/login" : location.pathname;
      };
    } else if (me.via === "token") {
      el.textContent = `Using token ${me.name}`;
    } else {
      el.innerHTML = `<a href="/devices#/security" title="Anyone who can reach this server can use it">No sign-in required</a>`;
    }
  } catch (e) { /* the status line already says */ }
}

function start({ pages, onRoute, onTick }) {
  Object.assign(App, { pages, onRoute, onTick });
  initTheme();
  showUser();
  window.addEventListener("hashchange", showPage);
  showPage();
  setInterval(() => { if (!document.hidden) refresh(); }, POLL_MS);
  document.addEventListener("visibilitychange", () => { if (!document.hidden) refresh(); });
}

// ---- theme: the system's, unless one is chosen ----

function initTheme() {
  const sel = $("theme");
  let saved = "";
  try { saved = localStorage.getItem("tephra.theme") || ""; } catch (e) { /* ignore */ }
  const apply = v => {
    if (v) document.documentElement.dataset.theme = v; else delete document.documentElement.dataset.theme;
  };
  apply(saved);
  if (!sel) return;
  sel.value = saved;
  sel.onchange = () => { try { localStorage.setItem("tephra.theme", sel.value); } catch (e) { /* ignore */ } apply(sel.value); };
}
