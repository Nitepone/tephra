//! The endpoints TRMNL firmware calls: setup, display and log, plus the images it downloads.
//! The protocol, and the traps these handlers avoid, are in `docs/` (firmware v1.8.19).

use crate::model::{self, Device, Telemetry};
use crate::{SharedState, display};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/setup", get(setup))
        .route("/api/display", get(display_handler))
        .route("/api/log", post(log))
        .route("/images/{file}", get(image))
}

fn header_str<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
    h.get(name).and_then(|v| v.to_str().ok()).map(str::trim).filter(|s| !s.is_empty())
}

fn telemetry(h: &HeaderMap) -> Telemetry {
    Telemetry {
        model: header_str(h, "Model").map(String::from),
        width: header_str(h, "Width").and_then(|v| v.parse().ok()),
        height: header_str(h, "Height").and_then(|v| v.parse().ok()),
        firmware: header_str(h, "FW-Version").map(String::from),
        battery_v: header_str(h, "Battery-Voltage").and_then(|v| v.parse().ok()),
        rssi: header_str(h, "RSSI").and_then(|v| v.parse().ok()),
    }
}

/// Where devices reach this server: the configured address, or the Host they used.
fn base_url(state: &SharedState, h: &HeaderMap) -> String {
    let url = state.cfg.public_url.trim_end_matches('/');
    if !url.is_empty() {
        return url.to_string();
    }
    format!("http://{}", header_str(h, "Host").unwrap_or("localhost"))
}

/// Internal failures are a plain HTTP 500. Never a JSON `status: 500`, which would reset the device.
fn internal(e: impl std::fmt::Display) -> Response {
    tracing::error!("device API: {e}");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// `GET /api/setup`: a device without credentials trades its MAC for an API key.
/// Any device that asks is registered (tephra is meant for a trusted network).
async fn setup(State(state): State<SharedState>, h: HeaderMap) -> Response {
    let Some(mac) = header_str(&h, "ID").map(str::to_uppercase) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let t = telemetry(&h);
    let result = (|| -> rusqlite::Result<Device> {
        let c = state.db.conn();
        let d = match model::device_by_mac(&c, &mac)? {
            Some(d) => d,
            None => {
                let d = model::register_device(
                    &c,
                    &mac,
                    t.model.as_deref().unwrap_or(""),
                    t.width.unwrap_or(800),
                    t.height.unwrap_or(480),
                    t.firmware.as_deref().unwrap_or(""),
                )?;
                model::log(&c, Some(d.id), "info", &format!("New device {mac} registered as {}", d.label()), None)?;
                d
            }
        };
        model::record_telemetry(&c, d.id, &t)?;
        Ok(d)
    })();
    match result {
        Ok(d) => Json(json!({
            "status": 200,
            "api_key": d.api_key.unwrap_or_default(),
            "friendly_id": d.friendly_id,
            "image_url": "",
            "message": "",
        }))
        .into_response(),
        Err(e) => internal(e),
    }
}

/// `GET /api/display`: what to show and how long to sleep.
async fn display_handler(State(state): State<SharedState>, h: HeaderMap) -> Response {
    let device = {
        let c = state.db.conn();
        let found = match header_str(&h, "Access-Token") {
            Some(key) => model::device_by_key(&c, key),
            None => Ok(None),
        };
        match found {
            Ok(Some(d)) => {
                if let Err(e) = model::record_telemetry(&c, d.id, &telemetry(&h)) {
                    return internal(e);
                }
                d
            }
            // A key we never issued, with a MAC: a device moved here from another server.
            Ok(None) if header_str(&h, "Access-Token").is_some() && header_str(&h, "ID").is_some() => {
                let mac = header_str(&h, "ID").unwrap_or_default().to_uppercase();
                let key = header_str(&h, "Access-Token").unwrap_or_default();
                let t = telemetry(&h);
                let adopted = model::adopt_device(
                    &c,
                    &mac,
                    key,
                    t.model.as_deref().unwrap_or(""),
                    t.width.unwrap_or(800),
                    t.height.unwrap_or(480),
                    t.firmware.as_deref().unwrap_or(""),
                )
                .and_then(|d| {
                    model::record_telemetry(&c, d.id, &t)?;
                    model::log(&c, Some(d.id), "info", &format!("Device {mac} joined with the key it already had, as {}", d.label()), None)?;
                    Ok(d)
                });
                match adopted {
                    Ok(d) => d,
                    Err(e) => return internal(e),
                }
            }
            // No key at all (a failed setup): a plain HTTP 404, so the firmware shows an error and asks again.
            Ok(None) => return StatusCode::NOT_FOUND.into_response(),
            Err(e) => return internal(e),
        }
    };
    let base = base_url(&state, &h);

    // A double-click of the button. `true` means the function configured for this device.
    let special = header_str(&h, "special_function").map(|v| if v == "true" { device.button.clone() } else { v.to_string() });
    let mut body = json!({
        "status": 0,
        "special_function": device.button,
        "reset_firmware": false,
        "update_firmware": false,
        "firmware_url": "",
    });
    let action = special.as_deref().map(|f| match f {
        "toggle_hold" | "rewind" => "guest_mode",
        f => f,
    });
    if let Some(a) = action {
        body["action"] = json!(a);
        // These want nothing but the action.
        if a == "send_to_me" || a == "add_wifi" {
            return Json(body).into_response();
        }
    }

    let png = if action == Some("identify") {
        display::identify_png(&device).map(|p| (p, None))
    } else {
        display::render_screen(&state, &device, None).await
    };
    let redraw_at = png.as_ref().ok().and_then(|(_, r)| *r);
    let image = png.and_then(|(png, _)| display::store_image(&state, &png));
    let refresh = display::refresh_s(&state, &device, redraw_at);
    match (image, refresh) {
        (Ok(name), Ok(refresh)) => {
            body["image_url"] = json!(format!("{base}/images/{name}.png"));
            body["filename"] = json!(name);
            body["refresh_rate"] = json!(refresh);
            Json(body).into_response()
        }
        (Err(e), _) => internal(format!("{e:#}")),
        (_, Err(e)) => internal(format!("{e:#}")),
    }
}

/// `POST /api/log`: the device's stored error entries. Always accepted, so the device can clear them.
async fn log(State(state): State<SharedState>, h: HeaderMap, body: Bytes) -> StatusCode {
    let c = state.db.conn();
    let device_id = header_str(&h, "Access-Token").and_then(|k| model::device_by_key(&c, k).ok().flatten()).map(|d| d.id);
    let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
    let entries = parsed.get("logs").and_then(|l| l.as_array()).cloned().unwrap_or_default();
    for e in &entries {
        let level = e.get("level").and_then(|v| v.as_str()).unwrap_or("error");
        let message = e.get("message").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| e.to_string());
        if let Err(err) = model::log(&c, device_id, level, &message, Some(&e.to_string())) {
            tracing::error!("storing a device log entry: {err}");
        }
    }
    StatusCode::NO_CONTENT
}

/// The rendered images, served as exactly `image/png`: the firmware compares the header verbatim.
async fn image(State(state): State<SharedState>, Path(file): Path<String>) -> Response {
    let valid = file.strip_suffix(".png").is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric()));
    if !valid {
        return StatusCode::NOT_FOUND.into_response();
    }
    match tokio::fs::read(state.cfg.data_dir.join("images").join(&file)).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, "image/png")], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
