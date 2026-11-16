// The Screens workspace: pick a device's screen, choose its layout, put an app in each slot and
// set the app up. The preview is the very image the device gets, rendered by the server.
"use strict";

const S = {
  presets: [],
  apps: [],
  instances: [],
  deviceId: null,      // the screen being edited
  screen: null,        // its saved layout: { preset, slots: [{ instance_id, title_bar }] }
  refreshS: null,
  slot: null,          // index of the selected slot
  drafts: new Map(),   // instance id -> { name, settings } not saved yet
  previewUrl: null,
};

const appById = id => S.apps.find(a => a.id === id);
const instById = id => S.instances.find(i => i.id === id);
const presetById = id => S.presets.find(p => p.id === id) || S.presets[0];

async function loadCatalog() {
  [S.presets, S.apps, S.instances] = await Promise.all([api("/presets"), api("/apps"), api("/instances")]);
}

// ---- the list of screens ----

function renderScreenList(r) {
  const current = String(S.deviceId);
  const items = App.devices.map(d =>
    `<li><a class="item ${String(d.id) === current ? "current" : ""}" href="${href("screens", [d.id])}">` +
    `<div class="line"><span class="grow">${esc(d.label)}</span>${d.placeholder ? `<span class="badge muted" title="Not connected: designed here, shown nowhere yet">placeholder</span>` : ""}</div>` +
    `<div class="sub"><span class="grow">${d.width}×${d.height}</span>${d.placeholder ? "" : ago(d.last_seen_ms)}</div></a></li>`).join("");
  setHtml("screen-list", items || `<li class="empty">No devices yet. <a href="/devices">Add a placeholder</a>, or point a TRMNL at this server.</li>`);
}

// ---- the editor ----

async function openScreen(id) {
  S.deviceId = id;
  S.slot = null;
  S.drafts.clear();
  if (id == null) { setHtml("screen-editor", ""); return; }
  const scr = await api(`/devices/${id}/screen`);
  S.screen = { preset: scr.preset, slots: scr.slots };
  S.refreshS = scr.refresh_s;
  renderEditor();
  updatePreview();
}

function device() { return App.devices.find(d => d.id === S.deviceId); }

function layoutIcon(p) {
  const rects = p.slots.map(([x, y, w, h]) =>
    `<rect x="${x * 40 + 1.5}" y="${y * 24 + 1.5}" width="${w * 40 - 3}" height="${h * 24 - 3}"/>`).join("");
  return `<svg viewBox="0 0 40 24" aria-hidden="true">${rects}</svg>`;
}

function renderEditor() {
  const d = device();
  if (!d || !S.screen) return;
  const p = presetById(S.screen.preset);
  const overlays = p.slots.map(([x, y, w, h], i) => {
    const inst = instById(S.screen.slots[i]?.instance_id);
    const label = inst ? inst.name : "Empty slot";
    return `<button type="button" class="slot ${i === S.slot ? "current" : ""}" data-slot="${i}" title="${esc(label)}" aria-label="Slot ${i + 1}: ${esc(label)}"
      style="left:${x * 100}%; top:${y * 100}%; width:${w * 100}%; height:${h * 100}%"></button>`;
  }).join("");
  const layouts = S.presets.map(q =>
    `<button type="button" class="${q.id === p.id ? "on" : ""}" data-preset="${q.id}" title="${esc(q.name)}" aria-label="${esc(q.name)}">${layoutIcon(q)}</button>`).join("");
  const refresh = S.refreshS ? `refreshes every ${fmtDuration(S.refreshS)}` : "";

  $("screen-editor").innerHTML = `
    <div class="page-head">
      <h1>${esc(d.label)}</h1>
      <span class="muted">${d.width}×${d.height}${d.placeholder ? " · placeholder" : ""}${refresh ? " · " + refresh : ""}</span>
      <span class="spacer"></span>
      <div class="layouts" role="group" aria-label="Layout">${layouts}</div>
    </div>
    <div class="editor">
      <div class="panel">
        <div class="panel-head"><h2>Preview</h2><span class="muted" id="preview-note"></span><span class="spacer"></span>
          <button type="button" class="small" id="preview-refresh">Render again</button></div>
        <div class="panel-body">
          <div class="preview" id="preview" style="aspect-ratio: ${d.width} / ${d.height}">
            <img id="preview-img" alt="What ${esc(d.label)} shows" width="${d.width}" height="${d.height}">${overlays}
          </div>
          <p class="hint">Exactly what the device gets. Click a slot to choose and set up its app.</p>
        </div>
      </div>
      <div id="slot-panel"></div>
    </div>`;

  for (const b of document.querySelectorAll(".layouts button")) b.onclick = () => setPreset(b.dataset.preset);
  for (const b of document.querySelectorAll(".preview .slot")) b.onclick = () => selectSlot(Number(b.dataset.slot));
  $("preview-refresh").onclick = () => updatePreview();
  renderSlotPanel();
}

async function saveScreen() {
  try {
    await apiSend("PUT", `/devices/${S.deviceId}/screen`, S.screen);
    S.refreshS = (await api(`/devices/${S.deviceId}/screen`)).refresh_s;
  } catch (e) { toast(e.message, "bad"); }
}

async function setPreset(id) {
  const n = presetById(id).slots.length;
  const slots = S.screen.slots.slice(0, n);
  while (slots.length < n) slots.push({ instance_id: null, title_bar: true });
  S.screen = { preset: id, slots };
  if (S.slot != null && S.slot >= n) S.slot = null;
  await saveScreen();
  renderEditor();
  updatePreview();
}

function selectSlot(i) {
  S.slot = S.slot === i ? null : i;
  for (const b of document.querySelectorAll(".preview .slot")) b.classList.toggle("current", Number(b.dataset.slot) === S.slot);
  renderSlotPanel();
}

// ---- the selected slot: which app, and its settings ----

function renderSlotPanel() {
  const box = $("slot-panel");
  if (!box) return;
  if (S.slot == null) {
    box.innerHTML = `<div class="panel"><div class="panel-body"><p class="empty">Click a slot in the preview to choose its app.</p></div></div>`;
    return;
  }
  const slot = S.screen.slots[S.slot];
  const inst = instById(slot.instance_id);
  const groups = S.apps.filter(a => !a.error).map(a => {
    const own = S.instances.filter(i => i.app === a.id)
      .map(i => `<option value="${i.id}" ${i.id === slot.instance_id ? "selected" : ""}>${esc(i.name)}</option>`).join("");
    return `<optgroup label="${esc(a.name)}">${own}<option value="new:${esc(a.id)}">New ${esc(a.name)}…</option></optgroup>`;
  }).join("");

  box.innerHTML = `
    <div class="panel">
      <div class="panel-head"><h2>Slot ${S.slot + 1}</h2><span class="spacer"></span>
        <button type="button" class="small" id="slot-close">Close</button></div>
      <div class="panel-body">
        <div class="form">
          <label for="slot-app">Shows</label>
          <select id="slot-app"><option value="">nothing</option>${groups}</select>
          <span class="label"></span>
          <label><input type="checkbox" id="slot-title" ${slot.title_bar ? "checked" : ""}> title bar</label>
        </div>
      </div>
    </div>
    <div id="instance-panel"></div>`;
  $("slot-close").onclick = () => selectSlot(S.slot);
  $("slot-app").onchange = e => chooseInstance(e.target.value);
  $("slot-title").onchange = async e => { slot.title_bar = e.target.checked; await saveScreen(); updatePreview(); };
  renderInstancePanel(inst);
}

async function chooseInstance(value) {
  const slot = S.screen.slots[S.slot];
  try {
    if (value.startsWith("new:")) {
      const app = appById(value.slice(4));
      const saved = await apiSend("POST", "/instances", { app: app.id, name: app.name, settings: {} });
      S.instances.push(saved.instance);
      slot.instance_id = saved.instance.id;
    } else {
      slot.instance_id = value ? Number(value) : null;
    }
    await saveScreen();
  } catch (e) { toast(e.message, "bad"); }
  renderEditor();
  updatePreview();
}

function fieldHtml(f, value) {
  const id = `set-${f.key}`;
  const help = f.help ? `<p class="hint">${esc(f.help)}</p>` : "";
  const label = esc(f.label || f.key);
  switch (f.type) {
    case "bool":
      return `<span class="label"></span><div><label><input type="checkbox" id="${id}" data-key="${esc(f.key)}" ${value ? "checked" : ""}> ${label}</label>${help}</div>`;
    case "choice":
      return `<label for="${id}">${label}</label><div><select id="${id}" data-key="${esc(f.key)}">` +
        f.options.map(o => `<option ${o === value ? "selected" : ""}>${esc(o)}</option>`).join("") + `</select>${help}</div>`;
    case "number":
      return `<label for="${id}">${label}</label><div><input type="number" step="any" id="${id}" data-key="${esc(f.key)}" value="${esc(value ?? "")}">${help}</div>`;
    case "text":
      return `<label for="${id}">${label}</label><div><textarea id="${id}" data-key="${esc(f.key)}" rows="4" cols="40">${esc(value ?? "")}</textarea>${help}</div>`;
    default:
      return `<label for="${id}">${label}</label><div><input type="text" id="${id}" data-key="${esc(f.key)}" value="${esc(value ?? "")}" size="40">${help}</div>`;
  }
}

function renderInstancePanel(inst) {
  const box = $("instance-panel");
  if (!box) return;
  if (!inst) { box.innerHTML = ""; return; }
  const app = appById(inst.app);
  const draft = S.drafts.get(inst.id) || { name: inst.name, settings: { ...inst.settings } };
  const fields = (app?.settings || []).map(f => fieldHtml(f, draft.settings[f.key] ?? f.default)).join("");
  const status = !app ? `<span class="bad">The app ${esc(inst.app)} is not installed.</span>`
    : inst.error ? `<span class="bad">Last fetch failed: ${esc(inst.error)}</span>`
    : app.has_fetch ? `Data fetched ${ago(inst.fetched_ms)}, every ${fmtDuration(app.refresh_s)}.` : "";

  box.innerHTML = `
    <div class="panel">
      <div class="panel-head"><h2>${esc(app?.name || inst.app)}</h2><span class="muted">${esc(app?.description || "")}</span></div>
      <div class="panel-body">
        <form class="form" id="inst-form" onsubmit="return false">
          <label for="inst-name">Name</label><input type="text" id="inst-name" value="${esc(draft.name)}" size="30">
          ${fields}
        </form>
        <div class="form-actions">
          <button type="button" class="primary" id="inst-save">Save</button>
          <button type="button" id="inst-revert">Revert</button>
          ${app?.has_fetch ? `<button type="button" id="inst-fetch">Fetch now</button>` : ""}
          <span class="dirty-note" id="inst-dirty" ${S.drafts.has(inst.id) ? "" : "hidden"}>Not saved. The preview shows your changes.</span>
        </div>
        <p class="hint" id="inst-status">${status}</p>
        <p class="hint">Changes apply on every screen that shows this instance. For different settings, choose New in the slot's list.</p>
      </div>
    </div>`;

  const form = $("inst-form");
  const readForm = () => {
    const settings = { ...inst.settings };
    for (const el of form.querySelectorAll("[data-key]")) {
      const f = app.settings.find(f => f.key === el.dataset.key);
      settings[el.dataset.key] = el.type === "checkbox" ? el.checked
        : f?.type === "number" ? (el.value === "" ? null : Number(el.value)) : el.value;
    }
    return { name: $("inst-name").value, settings };
  };
  const changed = () => {
    S.drafts.set(inst.id, readForm());
    $("inst-dirty").hidden = false;
    schedulePreview();
  };
  form.addEventListener("input", changed);
  form.addEventListener("change", changed);
  $("inst-save").onclick = () => saveInstance(inst.id, readForm());
  $("inst-revert").onclick = () => { S.drafts.delete(inst.id); renderInstancePanel(inst); updatePreview(); };
  if ($("inst-fetch")) $("inst-fetch").onclick = () => fetchInstance(inst.id);
}

async function saveInstance(id, body) {
  const btn = $("inst-save");
  btn.disabled = true;
  setText("inst-status", "Saving and fetching…");
  try {
    const r = await apiSend("PUT", `/instances/${id}`, body);
    S.drafts.delete(id);
    replaceInstance(r.instance);
    if (r.fetch_error) toast(`Saved, but the fetch failed: ${r.fetch_error}`, "bad");
    else toast("Saved.", "ok");
  } catch (e) { toast(e.message, "bad"); }
  renderSlotPanel();
  updatePreview();
}

async function fetchInstance(id) {
  setText("inst-status", "Fetching…");
  try {
    const r = await apiSend("POST", `/instances/${id}/fetch`);
    replaceInstance(r.instance);
    if (r.fetch_error) toast(`The fetch failed: ${r.fetch_error}`, "bad");
  } catch (e) { toast(e.message, "bad"); }
  renderInstancePanel(instById(id));
  updatePreview();
}

function replaceInstance(inst) {
  const i = S.instances.findIndex(x => x.id === inst.id);
  if (i >= 0) S.instances[i] = inst; else S.instances.push(inst);
}

// ---- the preview image ----

let previewTimer = null, previewSeq = 0;
function schedulePreview() {
  clearTimeout(previewTimer);
  previewTimer = setTimeout(updatePreview, 300);
}

async function updatePreview() {
  const img = $("preview-img");
  if (!img || S.deviceId == null) return;
  const seq = ++previewSeq;
  const settings = {};
  for (const [id, d] of S.drafts) settings[id] = d.settings;
  setText("preview-note", "rendering…");
  try {
    const r = S.drafts.size
      ? await fetch(`${API}/devices/${S.deviceId}/preview`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ screen: S.screen, settings }) })
      : await fetch(`${API}/devices/${S.deviceId}/screen.png`);
    if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || `HTTP ${r.status}`);
    const blob = await r.blob();
    if (seq !== previewSeq) return;  // a newer render is on its way
    if (S.previewUrl) URL.revokeObjectURL(S.previewUrl);
    S.previewUrl = URL.createObjectURL(blob);
    $("preview-img").src = S.previewUrl;
    setText("preview-note", `rendered ${new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`);
  } catch (e) {
    if (seq === previewSeq) setText("preview-note", "");
    toast(`Preview failed: ${e.message}`, "bad");
  }
}

// ---- the Apps page ----

function renderApps() {
  const apps = S.apps.map(a => `<tr>
      <td><b>${esc(a.name)}</b><div class="muted">${esc(a.description)}</div>${a.error ? `<div class="bad mono">${esc(a.error)}</div>` : ""}</td>
      <td class="mono">${esc(a.id)}</td>
      <td>${a.has_fetch ? "every " + fmtDuration(a.refresh_s) : "no fetch"}</td>
      <td class="num">${S.instances.filter(i => i.app === a.id).length}</td></tr>`).join("");
  const inst = S.instances.map(i => {
    const app = appById(i.app);
    const status = i.error ? `<span class="bad">${esc(i.error)}</span>` : app?.has_fetch ? (i.fetched_ms ? "ok" : "not fetched yet") : "";
    return `<tr>
      <td>${esc(i.name)}</td><td>${esc(app?.name || i.app)}</td>
      <td>${app?.has_fetch ? ago(i.fetched_ms) : ""}</td><td>${status}</td>
      <td class="nowrap">${app?.has_fetch ? `<button type="button" class="small" data-fetch="${i.id}">Fetch now</button> ` : ""}<button type="button" class="small danger" data-delete="${i.id}">Delete</button></td></tr>`;
  }).join("");
  setHtml("apps-body", `
    <div class="panel"><div class="panel-head"><h2>Installed apps</h2><span class="muted">One directory each under the apps directory, with an app.lua.</span></div>
      <div class="panel-body flush scroll-x"><table class="data">
        <thead><tr><th>App</th><th>Directory</th><th>Data</th><th class="num">Instances</th></tr></thead>
        <tbody>${apps || `<tr><td colspan="4" class="empty">No apps found.</td></tr>`}</tbody></table></div></div>
    <div class="panel"><div class="panel-head"><h2>Instances</h2><span class="muted">An app with its own settings. Create them from a slot on the Screens page.</span></div>
      <div class="panel-body flush scroll-x"><table class="data">
        <thead><tr><th>Name</th><th>App</th><th>Fetched</th><th>Status</th><th></th></tr></thead>
        <tbody>${inst || `<tr><td colspan="5" class="empty">None yet.</td></tr>`}</tbody></table></div></div>`);
}

$("apps-body").addEventListener("click", async e => {
  const f = e.target.closest("[data-fetch]"), d = e.target.closest("[data-delete]");
  try {
    if (f) {
      f.disabled = true;
      const r = await apiSend("POST", `/instances/${f.dataset.fetch}/fetch`);
      replaceInstance(r.instance);
      if (r.fetch_error) toast(`The fetch failed: ${r.fetch_error}`, "bad");
    }
    if (d) {
      const inst = instById(Number(d.dataset.delete));
      const ok = await confirmDialog({ title: "Delete instance", body: `<p>Delete <b>${esc(inst.name)}</b>? Slots that show it become empty.</p>`, action: "Delete", danger: true });
      if (!ok) return;
      await apiSend("DELETE", `/instances/${inst.id}`);
      S.instances = S.instances.filter(i => i.id !== inst.id);
    }
  } catch (err) { toast(err.message, "bad"); }
  renderApps();
});

$("apps-reload").onclick = async () => {
  try {
    S.apps = await apiSend("POST", "/apps/reload");
    const broken = S.apps.filter(a => a.error).length;
    toast(`${S.apps.length} apps loaded${broken ? `, ${broken} with errors` : ""}.`, broken ? "bad" : "ok");
  } catch (e) { toast(e.message, "bad"); }
  renderApps();
};

// ---- wiring ----

function fmtDuration(s) {
  if (s % 3600 === 0) return s === 3600 ? "hour" : `${s / 3600} hours`;
  if (s % 60 === 0) return s === 60 ? "minute" : `${s / 60} minutes`;
  return `${s} s`;
}

let catalogLoaded = loadCatalog().catch(e => toast(e.message, "bad"));

start({
  pages: ["screens", "apps"],
  onRoute: async r => {
    await catalogLoaded;
    if (r.page === "apps") {
      S.instances = await api("/instances").catch(() => S.instances);
      renderApps();
    }
  },
  onTick: async r => {
    await catalogLoaded;
    if (App.status) setText("foot-server", `tephra ${App.status.version}`);
    if (r.page !== "screens") return;
    const wanted = r.parts[0] != null ? Number(r.parts[0]) : App.devices[0]?.id ?? null;
    const exists = App.devices.some(d => d.id === wanted);
    if ((exists ? wanted : null) !== S.deviceId) await openScreen(exists ? wanted : null);
    renderScreenList(r);
  },
});
