// The Security page (Devices workspace): the sign-in lock, logins, browser sessions and access
// tokens. The server side is src/auth.rs.
"use strict";

const TOKEN_LIFETIMES = [[1, "1 hour"], [24, "24 hours"], [168, "7 days"], [720, "30 days"], [0, "No end"]];
let securityBuilt = false;
let sec = null;  // GET /security, as last fetched

async function renderSecurity() {
  const box = $("security-body");
  try {
    sec = await api("/security");
  } catch (e) {
    if (e.status === 403) { setHtml(box, `<div class="panel"><div class="panel-body">${esc(e.message)}</div></div>`); securityBuilt = false; return; }
    throw e;
  }
  if (!securityBuilt) buildSecurity(box);
  renderLock();
  renderTokens();
  renderUsers();
  renderSessions();
}

/// The page's frame and forms, once; polling only redraws the parts that show data.
function buildSecurity(box) {
  securityBuilt = true;
  box.__html = null;
  const lifetimes = TOKEN_LIFETIMES.map(([h, label]) => `<option value="${h}" ${h === 24 ? "selected" : ""}>${label}</option>`).join("");
  box.innerHTML = `
    <div class="panel"><div class="panel-head"><h2>Sign-in</h2></div><div id="sec-lock"></div></div>

    <div class="panel"><div class="panel-head"><h2>Access tokens</h2><span class="count" id="tok-count"></span></div>
      <div class="panel-body">
        <div class="toolbar">
          <button type="button" class="primary" id="tok-new">Make a link for an agent</button>
          <label>Lasts <select id="tok-hours">${lifetimes}</select></label>
          <label>Name <input type="text" id="tok-name" size="18" placeholder="Agent, with the date"></label>
        </div>
        <p class="hint">Makes a token and copies a link to give an agent: one page with the API docs and how to use the
          token. A token can do everything a login can, except manage this page. The link is shown only once.</p>
      </div>
      <div class="flush scroll-x" id="sec-tokens"></div></div>

    <div class="panel"><div class="panel-head"><h2>Logins</h2><span class="count" id="user-count"></span></div>
      <div class="flush scroll-x" id="sec-users"></div>
      <div class="panel-body">
        <form class="toolbar" id="user-form" onsubmit="return false">
          <label>Name <input type="text" id="user-name" autocomplete="off" size="16"></label>
          <label>Password <input type="password" id="user-pass" autocomplete="new-password" size="20" placeholder="at least 10 characters"></label>
          <button type="submit" id="user-add">Add login</button>
        </form>
      </div></div>

    <div class="panel"><div class="panel-head"><h2>Signed-in browsers</h2><span class="count" id="session-count"></span></div>
      <div class="flush scroll-x" id="sec-sessions"></div></div>`;
  $("tok-new").onclick = newToken;
  $("user-form").onsubmit = e => { e.preventDefault(); addUser(); };
}

function count(n, one, many) { return n ? `${n} ${n === 1 ? one : many}` : ""; }

function renderLock() {
  const s = sec;
  let action, title, hint;
  if (s.locked) {
    title = `<span class="dot ok"></span> Sign-in is required`;
    hint = "Pages and the API need a login or a token. Devices keep working: they have their own keys.";
    action = `<button type="button" id="lock-off" class="danger">Stop requiring…</button>`;
  } else {
    title = `<span class="dot bad"></span> Sign-in is not required`;
    hint = "Anyone who can reach this server can use and change it. The developer API still answers only this machine, or a token.";
    if (!s.users.length) action = `<span class="muted">Add a login below first</span>`;
    else if (!s.signed_in) action = `<a class="button" href="/login?next=${encodeURIComponent("/devices#/security")}">Sign in to turn on</a>`;
    else action = `<button type="button" id="lock-on" class="primary">Require sign-in</button>`;
  }
  setHtml("sec-lock", `
    <div class="setting"><div class="what"><div class="title">${title}</div><p class="hint">${hint}</p></div>${action}</div>
    <div class="setting"><div class="what"><div class="title">Accept new devices</div>
      <p class="hint">When off, a device this server hasn't seen gets no key when it sets up. Devices already added keep working.</p></div>
      <input type="checkbox" id="new-devices" aria-label="Accept new devices" ${s.new_devices ? "checked" : ""}></div>
    <div class="setting foot"><p class="hint">Locked out? Run <code>tephra --unlock</code> on the server.</p></div>`);
  if ($("lock-on")) $("lock-on").onclick = () => setLock(true);
  if ($("lock-off")) $("lock-off").onclick = async () => {
    if (await confirmDialog({ title: "Stop requiring sign-in", body: "<p>Anyone who can reach this server will be able to use and change it.</p>", action: "Stop requiring", danger: true })) setLock(false);
  };
  $("new-devices").onchange = async e => {
    try { await apiSend("PATCH", "/security", { new_devices: e.target.checked }); toast("Saved.", "ok"); }
    catch (err) { toast(err.message, "bad"); }
    renderSecurity();
  };
}

async function setLock(on) {
  try { await apiSend("PATCH", "/security", { locked: on }); toast(on ? "Sign-in is now required." : "Sign-in is no longer required.", "ok"); }
  catch (e) { toast(e.message, "bad"); }
  renderSecurity();
}

function expiry(ms) {
  if (ms == null) return "never";
  const left = ms - Date.now();
  const text = left < 3600e3 ? `in ${Math.max(1, Math.round(left / 60e3))} min` : left < 2 * 86400e3 ? `in ${Math.round(left / 3600e3)} h` : `in ${Math.round(left / 86400e3)} days`;
  return `<span title="${esc(fmtFull(ms))}">${text}</span>`;
}

function renderTokens() {
  const rows = sec.tokens.map(t => `<tr><td>${esc(t.name)}</td><td>${esc(t.created_by || "")}</td><td>${ago(t.created_ms)}</td>
    <td>${expiry(t.expires_ms)}</td><td>${ago(t.last_used_ms)}</td>
    <td class="actions"><button type="button" class="small danger" data-revoke="${t.id}">Revoke</button></td></tr>`).join("");
  setText("tok-count", count(sec.tokens.length, "token", "tokens"));
  if (setHtml("sec-tokens", !rows ? "" : `<table class="data"><thead><tr><th>Name</th><th>Made by</th><th>Made</th><th>Expires</th><th>Last used</th><th></th></tr></thead>
      <tbody>${rows}</tbody></table>`)) {
    for (const b of $("sec-tokens").querySelectorAll("[data-revoke]")) b.onclick = async () => {
      try { await apiSend("DELETE", `/security/tokens/${b.dataset.revoke}`); renderSecurity(); } catch (e) { toast(e.message, "bad"); }
    };
  }
}

async function newToken() {
  try {
    const { token, copied } = await makeAgentLink({ name: $("tok-name").value, hours: Number($("tok-hours").value) });
    if (copied) toast(`Copied a link for an agent. It works ${agentLinkUntil(token)}.`, "ok");
    else showAgentLink(token);
  } catch (e) { toast(e.message, "bad"); return; }
  $("tok-name").value = "";
  renderSecurity();
}

/// The link in a dialog, for copying by hand when the clipboard can't be written.
function showAgentLink(t) {
  const d = document.createElement("dialog");
  d.innerHTML = `<h2>Link for an agent</h2><div class="body">
      <p>Give this link to the agent. It is shown only now.</p>
      <textarea readonly rows="3" style="width:100%">${esc(t.url)}</textarea></div>
    <div class="actions"><button type="button" value="copy" class="primary">Copy</button><button type="button" value="done">Done</button></div>`;
  document.body.append(d);
  // Over plain HTTP (not localhost) browsers have no clipboard API; the older copy command
  // still works inside a click.
  d.querySelector('[value="copy"]').onclick = () => {
    d.querySelector("textarea").select();
    if (document.execCommand("copy")) { toast("Copied.", "ok"); d.close(); }
    else toast("Select the link and copy it by hand.", "bad");
  };
  d.querySelector('[value="done"]').onclick = () => d.close();
  d.addEventListener("close", () => d.remove());
  d.showModal();
  d.querySelector("textarea").select();
}

function renderUsers() {
  const rows = sec.users.map(u => `<tr><td>${esc(u.name)}</td><td>${ago(u.created_ms)}</td><td>${ago(u.last_login_ms)}</td>
    <td class="actions"><button type="button" class="small" data-pass="${u.id}">Change password…</button>
      <button type="button" class="small danger" data-remove="${u.id}">Remove…</button></td></tr>`).join("");
  setText("user-count", count(sec.users.length, "login", "logins"));
  $("user-add").className = sec.users.length ? "" : "primary";
  if (setHtml("sec-users", !rows ? `<div class="empty-line">No logins yet. Add one to be able to require sign-in.</div>` :
      `<table class="data"><thead><tr><th>Name</th><th>Added</th><th>Last sign-in</th><th></th></tr></thead><tbody>${rows}</tbody></table>`)) {
    for (const b of $("sec-users").querySelectorAll("[data-pass]")) b.onclick = () => changePassword(sec.users.find(u => u.id === Number(b.dataset.pass)));
    for (const b of $("sec-users").querySelectorAll("[data-remove]")) b.onclick = () => removeUser(sec.users.find(u => u.id === Number(b.dataset.remove)));
  }
}

async function addUser() {
  const name = $("user-name").value.trim(), password = $("user-pass").value;
  const first = !sec.users.length;
  try {
    await apiSend("POST", "/security/users", { name, password });
    // The first login signs this browser in, so the lock can be turned on next.
    if (first && !sec.signed_in) await apiSend("POST", "/auth/login", { name, password });
    $("user-name").value = ""; $("user-pass").value = "";
    toast(`Added ${name}.`, "ok");
    if (first && !sec.signed_in) { location.reload(); return; }
  } catch (e) { toast(e.message, "bad"); }
  renderSecurity();
}

function changePassword(u) {
  const d = document.createElement("dialog");
  d.innerHTML = `<h2>New password for ${esc(u.name)}</h2><div class="body">
      <input type="password" id="new-pass" autocomplete="new-password" placeholder="at least 10 characters" style="width:100%">
      <p class="hint">Signs ${esc(u.name)} out everywhere else.</p></div>
    <div class="actions"><button type="button" value="no">Cancel</button><button type="button" class="primary" value="yes">Change</button></div>`;
  document.body.append(d);
  d.querySelector('[value="no"]').onclick = () => d.close();
  d.querySelector('[value="yes"]').onclick = async () => {
    try { await apiSend("PUT", `/security/users/${u.id}`, { password: d.querySelector("#new-pass").value }); d.close(); toast("Password changed.", "ok"); renderSecurity(); }
    catch (e) { toast(e.message, "bad"); }
  };
  d.addEventListener("close", () => d.remove());
  d.showModal();
  d.querySelector("#new-pass").focus();
}

async function removeUser(u) {
  if (!await confirmDialog({ title: "Remove login", body: `<p>Remove <b>${esc(u.name)}</b> and sign it out everywhere?</p>`, action: "Remove", danger: true })) return;
  try { await apiSend("DELETE", `/security/users/${u.id}`); renderSecurity(); } catch (e) { toast(e.message, "bad"); }
}

/// "Firefox on Linux" from a User-Agent, roughly; the whole string is in the tooltip.
function browserName(agent) {
  const b = /Edg\//.test(agent) ? "Edge" : /Firefox\//.test(agent) ? "Firefox" : /Chrome\//.test(agent) ? "Chrome" : /Safari\//.test(agent) ? "Safari" : "";
  const os = /Android/.test(agent) ? "Android" : /iPhone|iPad/.test(agent) ? "iOS" : /Mac OS/.test(agent) ? "macOS" : /Windows/.test(agent) ? "Windows" : /Linux/.test(agent) ? "Linux" : "";
  return [b, os].filter(Boolean).join(" on ") || agent || "unknown";
}

function renderSessions() {
  const rows = sec.sessions.map(s => `<tr><td>${esc(s.user)}</td><td title="${esc(s.agent)}">${esc(browserName(s.agent))}${s.current ? ` <span class="badge">this browser</span>` : ""}</td>
    <td>${ago(s.created_ms)}</td><td>${ago(s.last_used_ms)}</td><td>${expiry(s.expires_ms)}</td>
    <td class="actions">${s.current ? "" : `<button type="button" class="small danger" data-end="${s.id}">Sign out</button>`}</td></tr>`).join("");
  setText("session-count", count(sec.sessions.length, "browser", "browsers"));
  if (setHtml("sec-sessions", !rows ? `<div class="empty-line">Nobody is signed in.</div>` :
      `<table class="data"><thead><tr><th>Login</th><th>Browser</th><th>Signed in</th><th>Last used</th><th>Expires</th><th></th></tr></thead><tbody>${rows}</tbody></table>`)) {
    for (const b of $("sec-sessions").querySelectorAll("[data-end]")) b.onclick = async () => {
      try { await apiSend("DELETE", `/security/sessions/${b.dataset.end}`); renderSecurity(); } catch (e) { toast(e.message, "bad"); }
    };
  }
}
