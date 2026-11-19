// The Devices workspace: every device (connected or placeholder), its health and setup; the log.
"use strict";

const MODELS = [
  { id: "og", name: "TRMNL OG", w: 800, h: 480 },
  { id: "x", name: "TRMNL X", w: 1872, h: 1404 },
  { id: "", name: "Other size", w: null, h: null },
];
const BUTTONS = [
  ["identify", "Show the device's name and ID"],
  ["send_to_me", "Show the current screen again"],
  ["add_wifi", "Open the Wi-Fi setup portal"],
  ["none", "Nothing"],
];

let shownDetail = null;  // device id whose form is on screen, so polling doesn't reset it

function renderDeviceList(r) {
  const current = String(selectedId(r));
  const items = App.devices.map(d =>
    `<li><a class="item ${String(d.id) === current ? "current" : ""}" href="${href("devices", [d.id])}">` +
    `<div class="line"><span class="grow">${esc(d.label)}</span>${d.placeholder ? `<span class="badge muted">placeholder</span>` : ""}</div>` +
    `<div class="sub"><span class="grow">${d.placeholder ? `${d.width}×${d.height}` : esc(d.mac)}</span>${d.placeholder ? "" : ago(d.last_seen_ms)}</div></a></li>`).join("");
  setHtml("device-list", items || `<li class="empty">No devices yet.</li>`);
}

function selectedId(r) {
  const id = r.parts[0] != null ? Number(r.parts[0]) : App.devices[0]?.id;
  return App.devices.some(d => d.id === id) ? id : App.devices[0]?.id ?? null;
}

function renderDeviceDetail(r, force) {
  const d = App.devices.find(d => d.id === selectedId(r));
  if (!d) { setHtml("device-facts", ""); $("device-form-box").innerHTML = ""; shownDetail = null; return; }
  renderFacts(d);
  if (!force && shownDetail === d.id) return;
  shownDetail = d.id;
  const buttons = BUTTONS.map(([v, label]) => `<option value="${v}" ${v === d.button ? "selected" : ""}>${esc(label)}</option>`).join("");
  $("device-form-box").innerHTML = `
    <div class="panel"><div class="panel-head"><h2>Settings</h2></div><div class="panel-body">
      <form class="form" id="device-form" onsubmit="return false">
        <label for="dev-name">Name</label><input type="text" id="dev-name" value="${esc(d.name)}" placeholder="${esc(d.label)}" size="30">
        ${d.placeholder ? `<label>Size</label><span><input type="number" id="dev-w" value="${d.width}" min="100" max="4000"> × <input type="number" id="dev-h" value="${d.height}" min="100" max="4000"> pixels</span>` : ""}
        <label for="dev-min">Refresh</label>
        <span>every <input type="number" id="dev-min" value="${d.refresh_min_s / 60}" min="1"> to <input type="number" id="dev-max" value="${d.refresh_max_s / 60}" min="1"> minutes
          <p class="hint">The device wakes as often as its most frequent app asks, within these bounds. Shorter drains the battery faster.</p></span>
        <label for="dev-grays">Grays</label>
        <span><select id="dev-grays">
            <option value="2" ${d.grays === 4 ? "" : "selected"}>2: black and white</option>
            <option value="4" ${d.grays === 4 ? "selected" : ""}>4: black, dark gray, light gray, white</option></select>
          <p class="hint">With 4, text gets gray edges and photos are dithered to four levels. A screen showing gray
            makes the display flash on every refresh (a full refresh), and needs firmware 1.6.0 or later. Apps can keep text
            black and white with <code>smooth = false</code>. A gray screen too large for the device to download is sent black and white.</p></span>
        ${d.placeholder ? "" : `<label for="dev-button">Double-click</label><select id="dev-button">${buttons}</select>`}
      </form>
      <div class="form-actions">
        <button type="button" class="primary" id="dev-save">Save</button>
        <a class="button" href="/#/screens/${d.id}">Edit its screen</a>
        <span class="spacer" style="flex:1"></span>
        <button type="button" class="danger" id="dev-delete">Delete…</button>
      </div>
    </div></div>`;
  $("dev-save").onclick = () => saveDevice(d);
  $("dev-delete").onclick = () => deleteDevice(d);
}

function renderFacts(d) {
  const row = (k, v) => `<dt>${esc(k)}</dt><dd>${v}</dd>`;
  const facts = d.placeholder
    ? row("Kind", "Placeholder: designed here, not connected to a device") + row("Display", `${d.width}×${d.height}, ${d.grays} grays`)
    : row("MAC", `<span class="mono">${esc(d.mac)}</span>`) +
      row("Friendly ID", `<span class="mono">${esc(d.friendly_id)}</span>`) +
      row("Model", esc(d.model || "unknown")) +
      row("Display", `${d.width}×${d.height}, ${d.grays} grays`) +
      row("Firmware", esc(d.firmware || "unknown")) +
      row("Battery", d.battery_v != null ? `${d.battery_v.toFixed(2)} V` : "unknown") +
      row("Wi-Fi", d.rssi != null ? `${d.rssi} dBm` : "unknown") +
      row("Last seen", ago(d.last_seen_ms));
  setHtml("device-facts", `<div class="page-head"><h1>${esc(d.label)}</h1></div>` +
    `<div class="panel"><div class="panel-head"><h2>Overview</h2></div><div class="panel-body"><dl class="facts">${facts}</dl></div></div>`);
}

async function saveDevice(d) {
  const body = {
    name: $("dev-name").value,
    refresh_min_s: Math.round(Number($("dev-min").value) * 60),
    refresh_max_s: Math.round(Number($("dev-max").value) * 60),
    grays: Number($("dev-grays").value),
  };
  if ($("dev-button")) body.button = $("dev-button").value;
  if ($("dev-w")) { body.width = Number($("dev-w").value); body.height = Number($("dev-h").value); }
  try {
    await apiSend("PATCH", `/devices/${d.id}`, body);
    toast("Saved.", "ok");
    await refresh();
    renderDeviceDetail(route(), true);
  } catch (e) { toast(e.message, "bad"); }
}

async function deleteDevice(d) {
  const body = d.placeholder
    ? `<p>Delete the placeholder <b>${esc(d.label)}</b> and its screen layout? The app instances stay.</p>`
    : `<p>Delete <b>${esc(d.label)}</b> and its screen layout? The app instances stay.</p><p>If the device is still running, it is registered again as a new device the next time it sets up.</p>`;
  if (!await confirmDialog({ title: "Delete device", body, action: "Delete", danger: true })) return;
  try {
    await apiSend("DELETE", `/devices/${d.id}`);
    location.hash = href("devices");
    await refresh();
  } catch (e) { toast(e.message, "bad"); }
}

function addDeviceDialog() {
  const d = document.createElement("dialog");
  const models = MODELS.map((m, i) => `<option value="${i}">${esc(m.name)}${m.w ? ` (${m.w}×${m.h})` : ""}</option>`).join("");
  d.innerHTML = `<h2>Add a placeholder device</h2>
    <div class="body"><p class="muted">A placeholder has a screen you can design without the hardware.</p>
      <form class="form" onsubmit="return false">
        <label for="new-name">Name</label><input type="text" id="new-name" size="24" placeholder="Kitchen">
        <label for="new-model">Model</label><select id="new-model">${models}</select>
        <label>Size</label><span><input type="number" id="new-w" value="800"> × <input type="number" id="new-h" value="480"></span>
      </form></div>
    <div class="actions"><button type="button" value="no">Cancel</button><button type="button" class="primary" value="yes">Add</button></div>`;
  document.body.append(d);
  const sync = () => {
    const m = MODELS[Number(d.querySelector("#new-model").value)];
    for (const k of ["w", "h"]) {
      const input = d.querySelector(`#new-${k}`);
      input.disabled = m[k] != null;
      if (m[k] != null) input.value = m[k];
    }
  };
  d.querySelector("#new-model").onchange = sync;
  sync();
  d.querySelector('[value="no"]').onclick = () => d.close();
  d.querySelector('[value="yes"]').onclick = async () => {
    const m = MODELS[Number(d.querySelector("#new-model").value)];
    try {
      const dev = await apiSend("POST", "/devices", {
        name: d.querySelector("#new-name").value || "Placeholder",
        model: m.id, width: Number(d.querySelector("#new-w").value), height: Number(d.querySelector("#new-h").value),
      });
      d.close();
      await refresh();
      location.hash = href("devices", [dev.id]);
    } catch (e) { toast(e.message, "bad"); }
  };
  d.addEventListener("close", () => d.remove());
  d.showModal();
  d.querySelector("#new-name").focus();
}
$("add-device").onclick = addDeviceDialog;

async function renderLog() {
  const entries = await api("/log?limit=300");
  const name = id => App.devices.find(d => d.id === id)?.label || (id ? `device ${id}` : "server");
  const rows = entries.map(e => `<tr class="${e.level === "error" || e.level === "fatal" ? "bad" : e.level === "warn" ? "warn" : ""}">
    <td class="when">${esc(new Date(e.at_ms).toLocaleString())}</td><td class="kind">${esc(name(e.device_id))}</td>
    <td class="kind">${esc(e.level)}</td><td class="what">${esc(e.message)}</td></tr>`).join("");
  setHtml("log-body", `<table class="events"><thead><tr><th>When</th><th>From</th><th>Level</th><th>Message</th></tr></thead>
    <tbody>${rows || `<tr><td colspan="4" class="empty">Nothing logged yet.</td></tr>`}</tbody></table>`);
}

start({
  pages: ["devices", "log", "security"],
  onRoute: () => { shownDetail = null; },
  onTick: async r => {
    if (App.status) setText("foot-server", `tephra ${App.status.version}`);
    if (r.page === "devices") { renderDeviceList(r); renderDeviceDetail(r); }
    if (r.page === "log") await renderLog();
    if (r.page === "security") await renderSecurity();
  },
});
