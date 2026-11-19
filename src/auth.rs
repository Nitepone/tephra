//! Who may use the server: logins, browser sessions and access tokens. Kept in this one file so
//! it can be read top to bottom.
//!
//! - **The lock** (`auth_enabled` in the settings table) is off on a new server. While it is
//!   off, everything is open, as before. When it is on, every route behind [`guard`] needs a
//!   signed-in browser session or a token. What devices call (`/api/setup`, `/api/display`,
//!   `/api/log`, `/images/…`) is never behind it: devices have their own keys.
//! - **Logins** are a name and a password, hashed with Argon2id. Signing in gives the browser a
//!   session cookie (HttpOnly, SameSite=Strict) for 30 days.
//! - **Tokens** are for scripts and agents: `Authorization: Bearer <token>` or `?token=<token>`.
//!   They last 24 hours unless made otherwise. A token can do anything a login can, except
//!   manage security (logins, tokens and the lock), so an agent cannot keep itself in.
//! - Session cookies and tokens are 32 random bytes; only their SHA-256 is stored.
//! - Locked out? `tephra --unlock` turns the lock off.

use crate::control_api::{ApiError, ApiResult};
use crate::db::now_ms;
use crate::SharedState;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post};
use axum::{Extension, Json, Router};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

const COOKIE: &str = "tephra_session";
const SESSION_MS: i64 = 30 * 24 * 3600 * 1000;
const TOKEN_PREFIX: &str = "tph_";
pub const DEFAULT_TOKEN_HOURS: i64 = 24;
const MIN_PASSWORD: usize = 10;

/// How the current request got in. Handlers read it with `Extension<Access>`.
#[derive(Clone, Debug)]
pub enum Access {
    /// The lock is off and the request carried no valid credentials.
    Open,
    /// A signed-in browser.
    User { id: i64, name: String, session: i64 },
    /// An access token.
    Token { id: i64, name: String },
}

impl Access {
    /// Logins, tokens and the lock: a signed-in user, or anyone while the lock is off.
    fn may_manage(&self) -> bool {
        !matches!(self, Access::Token { .. })
    }
    /// Carried credentials (rather than being let in because the lock is off).
    pub fn authenticated(&self) -> bool {
        !matches!(self, Access::Open)
    }
}

// ---- settings ----

fn setting(c: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    c.query_row("SELECT value FROM settings WHERE key = ?", [key], |r| r.get(0)).optional()
}
fn set_setting(c: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    c.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2", [key, value])?;
    Ok(())
}
pub fn lock_enabled(c: &Connection) -> rusqlite::Result<bool> {
    Ok(setting(c, "auth_enabled")?.as_deref() == Some("1"))
}
pub fn set_lock(c: &Connection, on: bool) -> rusqlite::Result<()> {
    set_setting(c, "auth_enabled", if on { "1" } else { "0" })
}
/// Whether `/api/setup` registers devices it hasn't seen. On by default.
pub fn new_devices_allowed(c: &Connection) -> rusqlite::Result<bool> {
    Ok(setting(c, "new_devices")?.as_deref() != Some("0"))
}

// ---- secrets ----

fn new_secret() -> String {
    use base64::Engine;
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).expect("system randomness");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}
fn digest(secret: &str) -> String {
    crate::apps::hex(&Sha256::digest(secret.as_bytes()))
}

fn hash_password(password: &str) -> Result<String, String> {
    use argon2::password_hash::{PasswordHasher, SaltString, rand_core::OsRng};
    argon2::Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|h| h.to_string())
        .map_err(|e| e.to_string())
}
fn password_matches(password: &str, hash: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    PasswordHash::new(hash).is_ok_and(|h| argon2::Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
}

// ---- the guard ----

fn cookie<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| kv.trim().strip_prefix(name)?.strip_prefix('='))
}

/// The token a request carries: an `Authorization: Bearer` header, or `token=` in the query.
fn carried_token(req: &Request) -> Option<String> {
    if let Some(v) = req.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        return v.strip_prefix("Bearer ").map(|t| t.trim().to_string());
    }
    query_token(req.uri())
}
fn query_token(uri: &axum::http::Uri) -> Option<String> {
    uri.query()?.split('&').find_map(|kv| kv.strip_prefix("token=")).map(String::from)
}

fn token_access(c: &Connection, token: &str) -> rusqlite::Result<Option<Access>> {
    let now = now_ms();
    let found: Option<(i64, String)> = c
        .query_row(
            "SELECT id, name FROM tokens WHERE hash = ? AND (expires_ms IS NULL OR expires_ms > ?)",
            params![digest(token), now],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, name)) = found else { return Ok(None) };
    c.execute("UPDATE tokens SET last_used_ms = ? WHERE id = ?", params![now, id])?;
    Ok(Some(Access::Token { id, name }))
}

fn session_access(c: &Connection, secret: &str) -> rusqlite::Result<Option<Access>> {
    let now = now_ms();
    let found: Option<(i64, i64, String)> = c
        .query_row(
            "SELECT s.id, u.id, u.name FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.hash = ? AND s.expires_ms > ?",
            params![digest(secret), now],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((session, id, name)) = found else { return Ok(None) };
    c.execute("UPDATE sessions SET last_used_ms = ? WHERE id = ?", params![now, session])?;
    Ok(Some(Access::User { id, name, session }))
}

/// A browser's request that changes something must come from this server's own pages. The
/// cookie is SameSite=Strict already; this also refuses a cross-origin `Origin`.
fn same_origin(h: &HeaderMap) -> bool {
    let Some(origin) = h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else { return true };
    let host = h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    origin.split_once("://").is_some_and(|(_, rest)| rest == host)
}

/// Lets a request through with its [`Access`], or turns it away: `401` for the API, the sign-in
/// page for anything else.
pub async fn guard(State(s): State<SharedState>, mut req: Request, next: Next) -> Response {
    let access = {
        let c = s.db.conn();
        let found = match carried_token(&req) {
            Some(t) => token_access(&c, &t),
            None => match cookie(req.headers(), COOKIE) {
                Some(secret) => session_access(&c, secret),
                None => Ok(None),
            },
        };
        let found = match found {
            Ok(f) => f,
            Err(e) => return ApiError::from(e).into_response(),
        };
        if let Some(Access::User { .. }) = &found
            && !matches!(*req.method(), Method::GET | Method::HEAD)
            && !same_origin(req.headers())
        {
            return (StatusCode::FORBIDDEN, Json(json!({ "error": "cross-origin request refused" }))).into_response();
        }
        match (found, lock_enabled(&c)) {
            (Some(a), _) => a,
            (None, Ok(false)) => Access::Open,
            (None, Ok(true)) => return turn_away(&req),
            (None, Err(e)) => return ApiError::from(e).into_response(),
        }
    };
    req.extensions_mut().insert(access);
    next.run(req).await
}

fn turn_away(req: &Request) -> Response {
    let path = req.uri().path();
    if path.starts_with("/api/") || path == "/agent" {
        let msg = "sign in, or send a token: Authorization: Bearer <token>";
        return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer")], Json(json!({ "error": msg }))).into_response();
    }
    // Only a local path goes back into `next`, so the sign-in page can't send anyone elsewhere.
    Redirect::to(&format!("/login?next={}", crate::dev::urlencode(path))).into_response()
}

// ---- routes ----

/// Signing in and out: reachable without credentials.
pub fn public_router() -> Router<SharedState> {
    Router::new()
        .route("/login", get(|| async { crate::web::login_page() }))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
}

/// Who is asking, and managing security: behind [`guard`], under `/api/v1`.
pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/auth/me", get(me))
        .route("/security", get(overview).patch(update_settings))
        .route("/security/users", post(add_user))
        .route("/security/users/{id}", axum::routing::put(set_password).delete(delete_user))
        .route("/security/sessions/{id}", delete(end_session))
        .route("/security/tokens", post(new_token))
        .route("/security/tokens/{id}", delete(revoke_token))
}

fn manager(a: &Access) -> ApiResult<()> {
    if a.may_manage() {
        Ok(())
    } else {
        Err(ApiError(StatusCode::FORBIDDEN, "tokens cannot manage logins, tokens or the lock; sign in".into()))
    }
}

#[derive(Deserialize)]
struct LoginIn {
    name: String,
    password: String,
}

async fn login(State(s): State<SharedState>, h: HeaderMap, Json(inp): Json<LoginIn>) -> ApiResult<Response> {
    if !same_origin(&h) {
        return Err(ApiError(StatusCode::FORBIDDEN, "cross-origin request refused".into()));
    }
    let found: Option<(i64, String)> = s
        .db
        .conn()
        .query_row("SELECT id, password_hash FROM users WHERE name = ?", [inp.name.trim()], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    // An unknown name costs the same as a wrong password, so names can't be told apart by timing.
    let (user, hash) = found.unwrap_or_else(|| (0, dummy_hash().to_string()));
    let ok = tokio::task::spawn_blocking(move || password_matches(&inp.password, &hash)).await.unwrap_or(false);
    if !ok || user == 0 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        return Err(ApiError(StatusCode::UNAUTHORIZED, "wrong name or password".into()));
    }
    let secret = new_secret();
    let now = now_ms();
    let agent = h.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("").chars().take(200).collect::<String>();
    {
        let c = s.db.conn();
        c.execute(
            "INSERT INTO sessions (hash, user_id, created_ms, expires_ms, last_used_ms, agent) VALUES (?, ?, ?, ?, ?, ?)",
            params![digest(&secret), user, now, now + SESSION_MS, now, agent],
        )?;
        c.execute("UPDATE users SET last_login_ms = ? WHERE id = ?", params![now, user])?;
    }
    let secure = if s.cfg.public_url.starts_with("https://") { "; Secure" } else { "" };
    let cookie = format!("{COOKIE}={secret}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{secure}", SESSION_MS / 1000);
    Ok(([(header::SET_COOKIE, cookie)], Json(json!({ "ok": true }))).into_response())
}

/// The hash of a random password nobody knows, checked against when the name is unknown.
fn dummy_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| hash_password(&new_secret()).expect("hashing"))
}

async fn logout(State(s): State<SharedState>, h: HeaderMap) -> ApiResult<Response> {
    if let Some(secret) = cookie(&h, COOKIE) {
        s.db.conn().execute("DELETE FROM sessions WHERE hash = ?", [digest(secret)])?;
    }
    let clear = format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0");
    Ok(([(header::SET_COOKIE, clear)], StatusCode::NO_CONTENT).into_response())
}

async fn me(State(s): State<SharedState>, Extension(a): Extension<Access>) -> ApiResult<Json<serde_json::Value>> {
    let locked = lock_enabled(&s.db.conn())?;
    let (via, name) = match &a {
        Access::Open => ("open", None),
        Access::User { name, .. } => ("login", Some(name.clone())),
        Access::Token { name, .. } => ("token", Some(name.clone())),
    };
    Ok(Json(json!({ "locked": locked, "via": via, "name": name, "may_manage": a.may_manage() })))
}

// ---- managing ----

#[derive(Serialize)]
struct UserOut {
    id: i64,
    name: String,
    created_ms: i64,
    last_login_ms: Option<i64>,
}
#[derive(Serialize)]
struct SessionOut {
    id: i64,
    user: String,
    created_ms: i64,
    expires_ms: i64,
    last_used_ms: Option<i64>,
    agent: String,
    current: bool,
}
#[derive(Serialize)]
struct TokenOut {
    id: i64,
    name: String,
    created_by: Option<String>,
    created_ms: i64,
    expires_ms: Option<i64>,
    last_used_ms: Option<i64>,
}

async fn overview(State(s): State<SharedState>, Extension(a): Extension<Access>) -> ApiResult<Json<serde_json::Value>> {
    manager(&a)?;
    let c = s.db.conn();
    let now = now_ms();
    // Expired credentials are of no use to anyone: drop them rather than list them.
    c.execute("DELETE FROM sessions WHERE expires_ms <= ?", [now])?;
    c.execute("DELETE FROM tokens WHERE expires_ms <= ?", [now])?;
    let users = c
        .prepare("SELECT id, name, created_ms, last_login_ms FROM users ORDER BY name")?
        .query_map([], |r| Ok(UserOut { id: r.get(0)?, name: r.get(1)?, created_ms: r.get(2)?, last_login_ms: r.get(3)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let current = if let Access::User { session, .. } = a { session } else { 0 };
    let sessions = c
        .prepare("SELECT s.id, u.name, s.created_ms, s.expires_ms, s.last_used_ms, s.agent FROM sessions s JOIN users u ON u.id = s.user_id ORDER BY s.last_used_ms DESC")?
        .query_map([], |r| {
            let id: i64 = r.get(0)?;
            Ok(SessionOut { id, user: r.get(1)?, created_ms: r.get(2)?, expires_ms: r.get(3)?, last_used_ms: r.get(4)?, agent: r.get(5)?, current: id == current })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let tokens = c
        .prepare("SELECT t.id, t.name, u.name, t.created_ms, t.expires_ms, t.last_used_ms FROM tokens t LEFT JOIN users u ON u.id = t.created_by ORDER BY t.created_ms DESC")?
        .query_map([], |r| {
            Ok(TokenOut { id: r.get(0)?, name: r.get(1)?, created_by: r.get(2)?, created_ms: r.get(3)?, expires_ms: r.get(4)?, last_used_ms: r.get(5)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Json(json!({
        "locked": lock_enabled(&c)?,
        "new_devices": new_devices_allowed(&c)?,
        "signed_in": matches!(a, Access::User { .. }),
        "users": users,
        "sessions": sessions,
        "tokens": tokens,
    })))
}

#[derive(Deserialize)]
struct SettingsIn {
    locked: Option<bool>,
    new_devices: Option<bool>,
}

async fn update_settings(State(s): State<SharedState>, Extension(a): Extension<Access>, Json(inp): Json<SettingsIn>) -> ApiResult<StatusCode> {
    manager(&a)?;
    let c = s.db.conn();
    if inp.locked == Some(true) && !matches!(a, Access::User { .. }) {
        return Err(ApiError::bad("sign in first, so turning the lock on doesn't lock you out"));
    }
    if let Some(on) = inp.locked {
        set_lock(&c, on)?;
        crate::model::log(&c, None, "info", &format!("Sign-in {} by {}", if on { "required" } else { "no longer required" }, who(&a)), None)?;
    }
    if let Some(on) = inp.new_devices {
        set_setting(&c, "new_devices", if on { "1" } else { "0" })?;
    }
    Ok(StatusCode::NO_CONTENT)
}

fn who(a: &Access) -> String {
    match a {
        Access::Open => "someone (the lock was off)".into(),
        Access::User { name, .. } => name.clone(),
        Access::Token { name, .. } => format!("token {name}"),
    }
}

#[derive(Deserialize)]
struct UserIn {
    #[serde(default)]
    name: String,
    password: String,
}

fn check_password(p: &str) -> ApiResult<()> {
    if p.chars().count() < MIN_PASSWORD {
        return Err(ApiError::bad(format!("passwords need at least {MIN_PASSWORD} characters")));
    }
    Ok(())
}

async fn hashed(password: String) -> ApiResult<String> {
    let h = tokio::task::spawn_blocking(move || hash_password(&password)).await.map_err(anyhow::Error::from)?;
    Ok(h.map_err(anyhow::Error::msg)?)
}

async fn add_user(State(s): State<SharedState>, Extension(a): Extension<Access>, Json(inp): Json<UserIn>) -> ApiResult<StatusCode> {
    manager(&a)?;
    let name = inp.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(ApiError::bad("a name of 1 to 64 characters"));
    }
    check_password(&inp.password)?;
    let hash = hashed(inp.password).await?;
    let c = s.db.conn();
    match c.execute("INSERT INTO users (name, password_hash, created_ms) VALUES (?, ?, ?)", params![name, hash, now_ms()]) {
        Ok(_) => {}
        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => {
            return Err(ApiError(StatusCode::CONFLICT, format!("there is already a login named {name}")));
        }
        Err(e) => return Err(e.into()),
    }
    crate::model::log(&c, None, "info", &format!("Login {name} added by {}", who(&a)), None)?;
    Ok(StatusCode::CREATED)
}

/// A new password. Ends the login's other sessions.
async fn set_password(State(s): State<SharedState>, Extension(a): Extension<Access>, Path(id): Path<i64>, Json(inp): Json<UserIn>) -> ApiResult<StatusCode> {
    manager(&a)?;
    check_password(&inp.password)?;
    let hash = hashed(inp.password).await?;
    let c = s.db.conn();
    if c.execute("UPDATE users SET password_hash = ? WHERE id = ?", params![hash, id])? == 0 {
        return Err(ApiError::not_found("login"));
    }
    let keep = if let Access::User { session, .. } = a { session } else { 0 };
    c.execute("DELETE FROM sessions WHERE user_id = ? AND id != ?", params![id, keep])?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_user(State(s): State<SharedState>, Extension(a): Extension<Access>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    manager(&a)?;
    let c = s.db.conn();
    let users: i64 = c.query_row("SELECT count(*) FROM users", [], |r| r.get(0))?;
    if users <= 1 && lock_enabled(&c)? {
        return Err(ApiError::bad("the last login can't be removed while sign-in is required"));
    }
    if c.execute("DELETE FROM users WHERE id = ?", [id])? == 0 {
        return Err(ApiError::not_found("login"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn end_session(State(s): State<SharedState>, Extension(a): Extension<Access>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    manager(&a)?;
    s.db.conn().execute("DELETE FROM sessions WHERE id = ?", [id])?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct TokenIn {
    #[serde(default)]
    name: String,
    /// How long it lasts; 0 for no end. 24 when left out.
    hours: Option<i64>,
}

/// A new token. Its secret is in this answer only: it is stored hashed.
async fn new_token(State(s): State<SharedState>, Extension(a): Extension<Access>, h: HeaderMap, Json(inp): Json<TokenIn>) -> ApiResult<Json<serde_json::Value>> {
    manager(&a)?;
    let hours = inp.hours.unwrap_or(DEFAULT_TOKEN_HOURS);
    if !(0..=24 * 366).contains(&hours) {
        return Err(ApiError::bad("hours: 0 (no end) up to a year"));
    }
    let now = now_ms();
    let expires = (hours > 0).then_some(now + hours * 3600 * 1000);
    let name = match inp.name.trim() {
        "" => format!("Agent {}", chrono::Local::now().format("%b %-d %H:%M")),
        n => n.chars().take(64).collect(),
    };
    let token = format!("{TOKEN_PREFIX}{}", new_secret());
    let creator = if let Access::User { id, .. } = a { Some(id) } else { None };
    let c = s.db.conn();
    c.execute(
        "INSERT INTO tokens (hash, name, created_by, created_ms, expires_ms) VALUES (?, ?, ?, ?, ?)",
        params![digest(&token), name, creator, now, expires],
    )?;
    let id = c.last_insert_rowid();
    crate::model::log(&c, None, "info", &format!("Token {name} made by {}", who(&a)), None)?;
    let base = crate::device_api::base_url(&s, &h);
    let url = format!("{base}/agent?token={token}");
    Ok(Json(json!({ "id": id, "name": name, "token": token, "expires_ms": expires, "url": url })))
}

async fn revoke_token(State(s): State<SharedState>, Extension(a): Extension<Access>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    manager(&a)?;
    s.db.conn().execute("DELETE FROM tokens WHERE id = ?", [id])?;
    Ok(StatusCode::NO_CONTENT)
}

fn fmt_time(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|t| t.with_timezone(&chrono::Local).format("%a %b %-d %H:%M %Z").to_string())
        .unwrap_or_default()
}

// ---- the agent brief ----

const BRIEF_FILES: [(&str, &str); 8] = [
    ("guide.md", include_str!("../luadoc/guide.md")),
    ("dev-api.md", include_str!("../luadoc/dev-api.md")),
    ("app.lua", include_str!("../luadoc/app.lua")),
    ("ui.lua", include_str!("../luadoc/ui.lua")),
    ("time.lua", include_str!("../luadoc/time.lua")),
    ("http.lua", include_str!("../luadoc/http.lua")),
    ("json.lua", include_str!("../luadoc/json.lua")),
    ("globals.lua", include_str!("../luadoc/globals.lua")),
];

/// `GET /agent`: everything an agent needs in one Markdown page: how to reach this server, the
/// guide, the developer API and the Lua reference (as its annotated source).
pub async fn agent_brief(State(s): State<SharedState>, Extension(a): Extension<Access>, h: HeaderMap, uri: axum::http::Uri) -> Response {
    let base = crate::device_api::base_url(&s, &h);
    let mut out = format!(
        "# tephra at {base}\n\n\
         tephra is a server that draws TRMNL e-ink screens from apps written in Lua. You were given this \
         page to write, test and save apps for it: it holds the guide, the developer API and the full \
         Lua reference.\n\n"
    );
    if let Access::Token { id, .. } = &a {
        let expires: Option<i64> = s.db.conn().query_row("SELECT expires_ms FROM tokens WHERE id = ?", [id], |r| r.get(0)).ok().flatten();
        // The link carries the token already; spell it out so it can go in a header.
        let token = query_token(&uri).unwrap_or_else(|| "<your token>".into());
        out += &format!(
            "**Access.** Send this header on every request: `Authorization: Bearer {token}` \
             (or add `?token={token}` to a URL). It works until {}, for everything below except \
             managing security.\n\n",
            expires.map(fmt_time).unwrap_or_else(|| "it is revoked".into())
        );
    }
    out += &format!(
        "The developer API is at {base}/api/v1/dev; the HTML version of this reference is at {base}/docs/lua/. \
         Example:\n\n    curl -s {base}/api/v1/dev/render -H \"Authorization: Bearer $TOKEN\" \\\n      \
         -H 'content-type: application/json' -d @render.json\n\n"
    );
    for (name, text) in BRIEF_FILES {
        if name.ends_with(".md") {
            out += &format!("\n---\n\n{text}\n");
        } else {
            out += &format!("\n---\n\n## Reference: {name}\n\n```lua\n{text}```\n");
        }
    }
    ([(header::CONTENT_TYPE, "text/markdown; charset=utf-8"), (header::CACHE_CONTROL, "no-store")], out).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dummy_hash_matches_nothing() {
        assert!(!password_matches("", dummy_hash()));
    }

    #[test]
    fn passwords_round_trip() {
        let h = hash_password("correct horse").unwrap();
        assert!(password_matches("correct horse", &h));
        assert!(!password_matches("correct horsE", &h));
    }

    #[test]
    fn cookies_are_found_by_exact_name() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, "a=1; tephra_session_x=no; tephra_session=yes".parse().unwrap());
        assert_eq!(cookie(&h, COOKIE), Some("yes"));
    }

    #[test]
    fn origin_must_match_host() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, "pi.local:8374".parse().unwrap());
        assert!(same_origin(&h));
        h.insert(header::ORIGIN, "http://pi.local:8374".parse().unwrap());
        assert!(same_origin(&h));
        h.insert(header::ORIGIN, "http://evil.example".parse().unwrap());
        assert!(!same_origin(&h));
    }
}
