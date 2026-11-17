// The Apps page of the Screens workspace: the list of apps and instances, the app editor
// (#/apps/app/<id>) and the instance editor (#/apps/instance/<id>).
// Shares S (apps, instances) with screens.js, which loads after it and starts the page.
"use strict";

const SIZES = [["full", "Full screen"], ["large", "⅔ slot"], ["half", "Half slot"], ["small", "Small slot"]];

/// Shows the Apps page for the route: the list, an app, or an instance.
async function showAppsPage(r) {
  const [kind, id] = r.parts;
  try {
    [S.apps, S.instances] = await Promise.all([api("/apps"), api("/instances")]);
  } catch (e) { toast(e.message, "bad"); }
  if (kind === "app" && id) return showAppEditor(id);
  if (kind === "instance" && id) return showInstanceEditor(Number(id));
  renderAppList();
}

function appLink(a) { return href("apps", ["app", a.id]); }
function instanceLink(i) { return href("apps", ["instance", i.id]); }

// ---- the list ----

function renderAppList() {
  const apps = S.apps.map(a => `<tr>
      <td><a href="${appLink(a)}"><b>${esc(a.name)}</b></a> ${a.builtin ? `<span class="badge muted">built-in</span>` : ""}${a.prototype ? `<span class="badge warn" title="Pushed by an agent through the developer API">prototype</span>` : ""}
        <div class="muted">${esc(a.description)}</div>${a.error ? `<div class="bad mono">${esc(a.error)}</div>` : ""}</td>
      <td>${a.has_fetch ? "every " + fmtDuration(a.refresh_s) : "no fetch"}</td>
      <td class="num">${a.instances}</td>
      <td class="nowrap"><button type="button" class="small" data-fork="${esc(a.id)}">Fork</button>
        <button type="button" class="small" data-export="${esc(a.id)}">Export</button></td></tr>`).join("");
  const inst = S.instances.map(i => {
    const app = appById(i.app);
    const status = i.error ? `<span class="bad">${esc(i.error)}</span>` : app?.has_fetch ? (i.fetched_ms ? "ok" : "not fetched yet") : "";
    return `<tr>
      <td><a href="${instanceLink(i)}">${esc(i.name)}</a></td>
      <td>${app ? `<a class="quiet" href="${appLink(app)}">${esc(app.name)}</a>` : esc(i.app)}</td>
      <td>${app?.has_fetch ? ago(i.fetched_ms) : ""}</td><td>${status}</td>
      <td class="nowrap">${app?.has_fetch ? `<button type="button" class="small" data-fetch="${i.id}">Fetch now</button> ` : ""}<button type="button" class="small danger" data-delete="${i.id}">Delete</button></td></tr>`;
  }).join("");
  $("apps-body").innerHTML = `
    <div class="page-head">
      <h1>Apps</h1><span class="spacer"></span>
      <button type="button" id="app-new" class="primary">New app</button>
      <button type="button" id="app-import" title="Choose an exported app file">Import</button>
      <button type="button" class="link" id="app-import-text">import from text</button>
      <button type="button" id="apps-reload" title="Read the apps directory again">Reload apps</button>
    </div>
    <div class="panel"><div class="panel-head"><h2>Apps</h2><span class="muted">Built-in apps come from the apps directory and can only be forked. The rest are yours.</span></div>
      <div class="panel-body flush scroll-x"><table class="data">
        <thead><tr><th>App</th><th>Data</th><th class="num">Instances</th><th></th></tr></thead>
        <tbody>${apps || `<tr><td colspan="4" class="empty">No apps.</td></tr>`}</tbody></table></div></div>
    <div class="panel"><div class="panel-head"><h2>Instances</h2><span class="muted">An app with its own config. Create them from a slot on the Screens page.</span></div>
      <div class="panel-body flush scroll-x"><table class="data">
        <thead><tr><th>Name</th><th>App</th><th>Fetched</th><th>Status</th><th></th></tr></thead>
        <tbody>${inst || `<tr><td colspan="5" class="empty">None yet.</td></tr>`}</tbody></table></div></div>`;

  $("app-new").onclick = async () => {
    try { const a = await apiSend("POST", "/apps"); location.hash = appLink(a); } catch (e) { toast(e.message, "bad"); }
  };
  $("app-import").onclick = importFromFile;
  $("app-import-text").onclick = importFromText;
  $("apps-reload").onclick = async () => {
    try {
      S.apps = await apiSend("POST", "/apps/reload");
      const broken = S.apps.filter(a => a.error).length;
      toast(`${S.apps.length} apps loaded${broken ? `, ${broken} with errors` : ""}.`, broken ? "bad" : "ok");
    } catch (e) { toast(e.message, "bad"); }
    renderAppList();
  };
}

$("apps-body").addEventListener("click", async e => {
  const b = e.target.closest("button[data-fork], button[data-export], button[data-fetch], button[data-delete]");
  if (!b) return;
  try {
    if (b.dataset.fork) return forkApp(b.dataset.fork);
    if (b.dataset.export) return exportApp(b.dataset.export);
    if (b.dataset.fetch) {
      b.disabled = true;
      const r = await apiSend("POST", `/instances/${b.dataset.fetch}/fetch`);
      replaceInstance(r.instance);
      if (r.fetch_error) toast(`The fetch failed: ${r.fetch_error}`, "bad");
    }
    if (b.dataset.delete) {
      const inst = instById(Number(b.dataset.delete));
      if (!await deleteInstance(inst)) return;
    }
  } catch (err) { toast(err.message, "bad"); }
  renderAppList();
});

// ---- fork, export, import ----

async function forkApp(id) {
  try {
    const a = await apiSend("POST", `/apps/${encodeURIComponent(id)}/fork`);
    toast(`Created ${a.name}.`, "ok");
    location.hash = appLink(a);
  } catch (e) { toast(e.message, "bad"); }
}

/// One click: downloads the base64 file, and copies it too where the browser allows that.
async function exportApp(id) {
  try {
    const { filename, data } = await api(`/apps/${encodeURIComponent(id)}/export`);
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([data], { type: "text/plain" }));
    a.download = filename;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
    let copied = false;
    try { if (navigator.clipboard && window.isSecureContext) { await navigator.clipboard.writeText(data); copied = true; } } catch (e) { /* not allowed */ }
    toast(`Exported to ${filename}${copied ? ", and copied" : ""}.`, "ok");
  } catch (e) { toast(e.message, "bad"); }
}

async function importData(data) {
  try {
    const a = await apiSend("POST", "/apps/import", { data });
    toast(`Imported ${a.name}.`, "ok");
    location.hash = appLink(a);
    return true;
  } catch (e) { toast(e.message, "bad"); return false; }
}

function importFromFile() {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = ".tephra-app,text/plain";
  input.onchange = async () => { if (input.files[0]) await importData(await input.files[0].text()); };
  input.click();
}

function importFromText() {
  const d = document.createElement("dialog");
  d.innerHTML = `<h2>Import an app</h2><div class="body"><p class="muted">Paste an exported app (base64 text).</p>
    <textarea rows="8" cols="60" class="mono" style="width:100%"></textarea></div>
    <div class="actions"><button type="button" value="no">Cancel</button><button type="button" class="primary" value="yes">Import</button></div>`;
  document.body.append(d);
  d.querySelector('[value="no"]').onclick = () => d.close();
  d.querySelector('[value="yes"]').onclick = async () => { if (await importData(d.querySelector("textarea").value)) d.close(); };
  d.addEventListener("close", () => d.remove());
  d.showModal();
  d.querySelector("textarea").focus();
}

async function deleteInstance(inst) {
  const ok = await confirmDialog({ title: "Delete instance", body: `<p>Delete <b>${esc(inst.name)}</b>? Slots that show it become empty.</p>`, action: "Delete", danger: true });
  if (!ok) return false;
  await apiSend("DELETE", `/instances/${inst.id}`);
  S.instances = S.instances.filter(i => i.id !== inst.id);
  return true;
}

// ---- trying an app: run it once, on its own, and show what it draws ----

function tryPanel(title) {
  const sizes = SIZES.map(([v, l]) => `<option value="${v}">${esc(l)}</option>`).join("");
  return `<div class="panel"><div class="panel-head"><h2>${esc(title)}</h2>
      <span class="muted">Runs the config, fetch and render once, as the code stands. Nothing is saved.</span></div>
    <div class="panel-body try-box">
      <div class="stack">
        <div class="toolbar"><label>Size <select id="try-size">${sizes}</select></label>
          <button type="button" id="try-run" class="primary">Run</button><span class="muted" id="try-note"></span></div>
        <div id="try-config-box"></div>
      </div>
      <div><img id="try-img" alt="What the app draws" width="800" height="480" hidden></div>
    </div></div>`;
}

let tryUrl = null;
async function runTry(appId, body) {
  setText("try-note", "running…");
  $("try-run").disabled = true;
  try {
    const r = await fetch(`${API}/apps/${encodeURIComponent(appId)}/try`, {
      method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body),
    });
    if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || `HTTP ${r.status}`);
    if (tryUrl) URL.revokeObjectURL(tryUrl);
    tryUrl = URL.createObjectURL(await r.blob());
    $("try-img").src = tryUrl;
    $("try-img").hidden = false;
    setText("try-note", `ran ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" })}`);
  } catch (e) {
    setText("try-note", "");
    toast(`Run failed: ${e.message}`, "bad");
  } finally {
    $("try-run").disabled = false;
  }
}

// ---- the app editor ----

function showAppEditor(id) {
  const app = appById(id);
  if (!app) { $("apps-body").innerHTML = `<p class="empty">No such app. <a href="${href("apps")}">All apps</a></p>`; return; }
  const ro = app.builtin;
  $("apps-body").innerHTML = `
    <div class="page-head">
      <a href="${href("apps")}">Apps</a><span class="muted">/</span>
      <h1>${esc(app.name)}</h1>${ro ? `<span class="badge muted">built-in</span>` : ""}${app.prototype ? `<span class="badge warn" id="app-proto-badge">prototype</span>` : ""}
      <span class="spacer"></span>
      <button type="button" id="app-fork">Create copy (fork)</button>
      <button type="button" id="app-export">Export</button>
      ${ro ? "" : `<button type="button" class="danger" id="app-delete">Delete…</button>`}
    </div>
    ${ro ? `<div class="banner" style="border: 1px solid var(--notice-rule); margin-bottom: 14px"><span>Built-in apps can't be edited here.
      <b>Create copy</b> makes your own version to change.</span></div>` : ""}
    <div class="panel"><div class="panel-body">
      <form class="form" onsubmit="return false">
        <label for="app-name">Name</label><input type="text" id="app-name" value="${esc(app.name)}" size="40" ${ro ? "disabled" : ""}>
        <label for="app-desc">Description</label><input type="text" id="app-desc" value="${esc(app.description)}" size="80" ${ro ? "disabled" : ""}>
        ${app.prototype ? `<label for="app-prototype">Prototype</label><label class="hint"><input type="checkbox" id="app-prototype" checked>
          Pushed by an agent through the developer API. Untick and save to keep it as a finished app.</label>` : ""}
      </form>
    </div></div>
    <div class="panel">
      <div class="panel-head"><h2>Code</h2><span class="muted">Lua · <a href="/docs/lua/" target="_blank" rel="noopener">API reference</a></span></div>
      <div class="panel-body stack">
        <div id="app-code"></div>
        <p class="hint bad mono" id="app-error" ${app.error ? "" : "hidden"}>${esc(app.error || "")}</p>
        <label><input type="checkbox" id="app-validate-on" ${app.validate_enabled ? "checked" : ""} ${ro ? "disabled" : ""}>
          Check each instance's config before the app runs</label>
        <div id="app-validate-box" ${app.validate_enabled ? "" : "hidden"}>
          <p class="hint">Runs after the config, with it in <code>config</code>. Stop a bad config with <code>assert(ok, "message")</code> or <code>error("message")</code>.</p>
          <div id="app-validate"></div>
        </div>
      </div>
    </div>
    ${ro ? "" : `<div class="form-actions" style="margin: -4px 0 14px">
      <button type="button" class="primary" id="app-save">Save</button>
      <button type="button" id="app-revert">Revert</button>
      <span class="dirty-note" id="app-dirty" hidden>Not saved.</span></div>`}
    ${tryPanel("Try it")}`;

  const dirty = () => { if ($("app-dirty")) $("app-dirty").hidden = false; };
  const code = codeEditor($("app-code"), { value: app.code, readOnly: ro, minLines: 12, label: "App code", onInput: dirty });
  const check = codeEditor($("app-validate"), { value: app.validate_code, readOnly: ro, minLines: 3, label: "Config check", onInput: dirty });
  const config = codeEditor($("try-config-box"), { value: app.config_template, minLines: 4, label: "Config to try" });
  $("app-validate-on").onchange = e => { $("app-validate-box").hidden = !e.target.checked; dirty(); };
  if (!ro) {
    $("app-name").oninput = dirty;
    $("app-desc").oninput = dirty;
    if ($("app-prototype")) $("app-prototype").onchange = dirty;
  }
  $("app-fork").onclick = () => forkApp(app.id);
  $("app-export").onclick = () => exportApp(app.id);
  if ($("app-delete")) $("app-delete").onclick = async () => {
    if (app.instances) { toast(`Delete its ${app.instances} instance${app.instances === 1 ? "" : "s"} first.`, "bad"); return; }
    if (!await confirmDialog({ title: "Delete app", body: `<p>Delete <b>${esc(app.name)}</b>? Export it first to keep a copy.</p>`, action: "Delete", danger: true })) return;
    try { await apiSend("DELETE", `/apps/${encodeURIComponent(app.id)}`); location.hash = href("apps"); } catch (e) { toast(e.message, "bad"); }
  };
  if ($("app-save")) $("app-save").onclick = async () => {
    try {
      const saved = await apiSend("PUT", `/apps/${encodeURIComponent(app.id)}`, {
        name: $("app-name").value, description: $("app-desc").value, code: code.value,
        validate_code: check.value, validate_enabled: $("app-validate-on").checked,
        ...($("app-prototype") ? { prototype: $("app-prototype").checked } : {}),
      });
      Object.assign(app, saved);
      $("app-dirty").hidden = true;
      $("app-error").hidden = !saved.error;
      setText("app-error", saved.error || "");
      document.querySelector("#apps-body h1").textContent = saved.name;
      if ($("app-proto-badge")) $("app-proto-badge").hidden = !saved.prototype;
      toast(saved.error ? "Saved, but the code doesn't load. See the error under it." : "Saved.", saved.error ? "bad" : "ok");
    } catch (e) { toast(e.message, "bad"); }
  };
  if ($("app-revert")) $("app-revert").onclick = () => showAppEditor(id);
  $("try-run").onclick = () => runTry(app.id, {
    config: config.value, size: $("try-size").value,
    ...(ro ? {} : { code: code.value, validate_code: check.value, validate_enabled: $("app-validate-on").checked }),
  });
}

// ---- the instance editor ----

function settingsReference(app) {
  if (!app?.settings?.length) return "";
  const rows = app.settings.map(f => `<tr><td class="mono">${esc(f.key)}</td><td class="mono">${f.default == null ? "" : esc(JSON.stringify(f.default))}</td>
    <td>${esc(f.label || "")}${f.options?.length ? ` (one of: ${esc(f.options.join(", "))})` : ""}${f.help ? `. ${esc(f.help)}` : ""}</td></tr>`).join("");
  return `<details><summary class="muted">What this app's config may set</summary>
    <table class="data" style="margin-top: 6px"><thead><tr><th>Name</th><th>Default</th><th>About</th></tr></thead><tbody>${rows}</tbody></table></details>`;
}

function showInstanceEditor(id) {
  const inst = instById(id);
  if (!inst) { $("apps-body").innerHTML = `<p class="empty">No such instance. <a href="${href("apps")}">All apps</a></p>`; return; }
  const app = appById(inst.app);
  const status = !app ? `<span class="bad">The app ${esc(inst.app)} is not installed.</span>`
    : inst.error ? `<span class="bad">Last fetch failed: ${esc(inst.error)}</span>`
    : app.has_fetch ? `Data fetched ${ago(inst.fetched_ms)}, every ${fmtDuration(app.refresh_s)}.` : "";
  $("apps-body").innerHTML = `
    <div class="page-head">
      <a href="${href("apps")}">Apps</a><span class="muted">/</span>
      <h1>${esc(inst.name)}</h1>
      <span class="muted">an instance of ${app ? `<a href="${appLink(app)}">${esc(app.name)}</a>` : esc(inst.app)}</span>
      <span class="spacer"></span>
      <button type="button" class="danger" id="inst-delete">Delete…</button>
    </div>
    <div class="panel"><div class="panel-body stack">
      <form class="form" onsubmit="return false">
        <label for="inst-name">Name</label><input type="text" id="inst-name" value="${esc(inst.name)}" size="40">
      </form>
      <div>
        <h3>Config</h3>
        <p class="hint" style="margin-bottom: 6px">Lua, run before the app. What it assigns is the app's <code>config</code>; anything
          left out takes the app's default.</p>
        <div id="inst-config"></div>
      </div>
      ${settingsReference(app)}
      <div class="form-actions">
        <button type="button" class="primary" id="inst-save">Save</button>
        <button type="button" id="inst-revert">Revert</button>
        ${app?.has_fetch ? `<button type="button" id="inst-fetch">Fetch now</button>` : ""}
        <button type="button" id="inst-template" title="Replace the config with the app's defaults">Start over from the defaults</button>
        <span class="dirty-note" id="inst-dirty" hidden>Not saved.</span>
      </div>
      <p class="hint" id="inst-status">${status}</p>
    </div></div>
    ${app ? tryPanel("Try this config") : ""}`;

  const dirty = () => { $("inst-dirty").hidden = false; };
  const config = codeEditor($("inst-config"), { value: inst.config, minLines: 6, label: "Instance config", onInput: dirty });
  $("inst-name").oninput = dirty;
  $("inst-save").onclick = async () => {
    try {
      const r = await apiSend("PUT", `/instances/${inst.id}`, { name: $("inst-name").value, config: config.value });
      replaceInstance(r.instance);
      Object.assign(inst, r.instance);
      $("inst-dirty").hidden = true;
      document.querySelector("#apps-body h1").textContent = r.instance.name;
      if (r.fetch_error) toast(`Saved, but the fetch failed: ${r.fetch_error}`, "bad"); else toast("Saved.", "ok");
    } catch (e) { toast(e.message, "bad"); }
  };
  $("inst-revert").onclick = () => showInstanceEditor(id);
  $("inst-template").onclick = () => { config.value = app?.config_template || ""; dirty(); };
  if ($("inst-fetch")) $("inst-fetch").onclick = async () => {
    setText("inst-status", "Fetching…");
    try {
      const r = await apiSend("POST", `/instances/${inst.id}/fetch`);
      replaceInstance(r.instance);
      if (r.fetch_error) toast(`The fetch failed: ${r.fetch_error}`, "bad"); else toast("Fetched.", "ok");
    } catch (e) { toast(e.message, "bad"); }
    showInstanceEditor(id);
  };
  $("inst-delete").onclick = async () => {
    try { if (await deleteInstance(inst)) location.hash = href("apps"); } catch (e) { toast(e.message, "bad"); }
  };
  if (app) {
    $("try-config-box").innerHTML = `<p class="hint">Uses the config above, saved or not.</p>`;
    $("try-run").onclick = () => runTry(app.id, { config: config.value, size: $("try-size").value });
  }
}
