// Making a link for an AI agent: a new token (POST /api/v1/security/tokens) and its link to
// /agent, one page with the docs and how to use the token. Stands alone (no ui.js), so the Lua
// reference can load it too: there it fills <div id="agent-link"> with a button.
"use strict";

/// Makes a token and copies its link. Resolves to { token, copied }; `token` is the server's
/// answer ({ url, expires_ms, … }). `opts`: { name, hours }.
/// The clipboard is written through a pending ClipboardItem where the browser has one, so it
/// still counts as part of the click after waiting for the server. Over plain HTTP (other than
/// localhost) browsers have no clipboard API at all, and `copied` is false.
async function makeAgentLink(opts) {
  const made = fetch("/api/v1/security/tokens", {
    method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(opts || {}),
  }).then(async r => {
    const body = await r.json().catch(() => ({}));
    if (r.status === 401) throw new Error("Sign in to make a link.");
    if (!r.ok) throw new Error(body.error || `${r.status} ${r.statusText}`);
    return body;
  });
  let copied = false;
  try {
    if (window.ClipboardItem && navigator.clipboard?.write) {
      const blob = made.then(t => new Blob([t.url], { type: "text/plain" }));
      await navigator.clipboard.write([new ClipboardItem({ "text/plain": blob })]);
    } else {
      await navigator.clipboard.writeText((await made).url);
    }
    copied = true;
  } catch (e) { /* not copied; the caller shows the link */ }
  return { token: await made, copied };
}

/// "until Thu, 19 Nov, 10:12", or "until revoked".
function agentLinkUntil(t) {
  return t.expires_ms ? "until " + new Date(t.expires_ms).toLocaleString(undefined, { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }) : "until revoked";
}

// The Lua reference's button.
(function () {
  const box = document.getElementById("agent-link");
  if (!box) return;
  box.innerHTML = `<p><button type="button" style="font: inherit; padding: 4px 12px; cursor: pointer">Make a link for an agent</button>
    <span role="status"></span></p><p hidden><input readonly style="width: 100%; font-family: monospace; padding: 4px"></p>`;
  const [button, status, out, input] = ["button", "span", "p[hidden]", "input"].map(s => box.querySelector(s));
  button.onclick = async () => {
    button.disabled = true;
    status.textContent = " Making…";
    try {
      const { token, copied } = await makeAgentLink();
      input.value = token.url;
      out.hidden = false;
      input.select();
      status.textContent = ` ${copied ? "Copied to the clipboard." : "Copy it from the box below."} It works ${agentLinkUntil(token)}.`;
    } catch (e) {
      status.textContent = " " + e.message;
    } finally {
      button.disabled = false;
    }
  };
})();
