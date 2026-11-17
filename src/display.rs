//! Producing a device's image: gathering its screen, running each slot's app, composing,
//! and keeping app data fresh. The web preview and the device both come through here, so
//! they get the same pixels.

use crate::apps::{self, Slot};
use crate::model::{self, Device, ScreenConfig};
use crate::render::layout::Node;
use crate::screen::{self, SlotDraw};
use crate::{SharedState, db::now_ms};
use anyhow::Context;
use std::collections::HashMap;
use std::time::Duration;

/// Unsaved edits the screen editor wants to see: a different layout, and instance configs not
/// yet saved.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Draft {
    pub screen: Option<ScreenConfig>,
    #[serde(default)]
    pub configs: HashMap<i64, String>,
}

/// The device's screen as the PNG it downloads.
pub async fn render_png(state: &SharedState, device: &Device, draft: Option<Draft>) -> anyhow::Result<Vec<u8>> {
    let draft = draft.unwrap_or_default();
    let (cfg, instances) = {
        let c = state.db.conn();
        let cfg = match draft.screen {
            Some(s) => s,
            None => model::screen(&c, device.id)?,
        };
        let mut instances = HashMap::new();
        for id in cfg.slots.iter().filter_map(|s| s.instance_id) {
            if let Some(mut i) = model::instance(&c, id)? {
                if let Some(c) = draft.configs.get(&id) {
                    i.config = c.clone();
                }
                instances.insert(id, i);
            }
        }
        (cfg, instances)
    };
    let state = state.clone();
    let device = device.clone();
    tokio::task::spawn_blocking(move || {
        let p = screen::preset(&cfg.preset);
        let title_bars: Vec<bool> = cfg.slots.iter().map(|s| s.title_bar && s.instance_id.is_some()).collect();
        let geoms = screen::geometry(p, device.width, device.height, &title_bars);
        if cfg.slots.iter().all(|s| s.instance_id.is_none()) {
            return welcome(&state, &device).to_png();
        }
        let slots: Vec<SlotDraw> = geoms
            .iter()
            .zip(&cfg.slots)
            .map(|(g, s)| {
                let Some(inst) = s.instance_id.and_then(|id| instances.get(&id)) else {
                    return SlotDraw { geom: *g, content: None, title: String::new() };
                };
                let content = match state.apps.get(&inst.app) {
                    None => Err(format!("the app {:?} is not installed", inst.app)),
                    Some(app) => {
                        let slot = Slot { w: g.content.w, h: g.content.h, size: g.size, grays: device.grays };
                        apps::render(&app, &inst.config, &inst.data, slot, &state.cfg.data_dir)
                    }
                };
                SlotDraw { geom: *g, content: Some(content), title: inst.name.clone() }
            })
            .collect();
        screen::compose(device.width, device.height, device.grays, p.slots.len() > 1, &slots).to_device_png(crate::render::OG_IMAGE_LIMIT)
    })
    .await?
}

fn welcome(state: &SharedState, d: &Device) -> crate::render::Canvas {
    let url = state.cfg.public_url.trim_end_matches('/');
    let mut lines = vec![Node::text(d.label(), "large"), Node::text("This screen has no apps yet.", "title")];
    lines.push(Node::text(
        if url.is_empty() { "Choose some in tephra's Screens workspace.".to_string() } else { format!("Choose some at {url}") },
        "body",
    ));
    screen::message_screen(d.width, d.height, lines)
}

/// The screen shown when the button asks a device to identify itself.
pub fn identify_png(d: &Device) -> anyhow::Result<Vec<u8>> {
    let mut lines = vec![Node::text(d.label(), "huge")];
    if !d.friendly_id.is_empty() {
        lines.push(Node::text(format!("ID {}", d.friendly_id), "title"));
    }
    if let Some(mac) = &d.mac {
        lines.push(Node::text(mac.clone(), "body"));
    }
    screen::message_screen(d.width, d.height, lines).to_png()
}

/// The refresh interval to hand the device: the most frequent app on its screen, within the
/// device's bounds.
pub fn refresh_s(state: &SharedState, device: &Device) -> anyhow::Result<u64> {
    let c = state.db.conn();
    let cfg = model::screen(&c, device.id)?;
    let mut shortest = None::<u64>;
    for id in cfg.slots.iter().filter_map(|s| s.instance_id) {
        if let Some(app) = model::instance(&c, id)?.and_then(|i| state.apps.get(&i.app)) {
            shortest = Some(shortest.map_or(app.refresh_s, |s| s.min(app.refresh_s)));
        }
    }
    let lo = (device.refresh_min_s as u64).max(60);
    let hi = (device.refresh_max_s as u64).max(lo);
    Ok(shortest.unwrap_or(hi).clamp(lo, hi))
}

/// Writes the PNG under its content hash (once) and returns the file name without extension.
pub fn store_image(state: &SharedState, png: &[u8]) -> anyhow::Result<String> {
    use sha2::Digest;
    let name = format!("t{}", &apps::hex(&sha2::Sha256::digest(png))[..20]);
    let dir = state.cfg.data_dir.join("images");
    let path = dir.join(format!("{name}.png"));
    if path.exists() {
        // Still in use: keep it from being pruned.
        std::fs::File::options().write(true).open(&path)?.set_modified(std::time::SystemTime::now())?;
    } else {
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let tmp = dir.join(format!("{name}.tmp"));
        std::fs::write(&tmp, png)?;
        std::fs::rename(&tmp, &path)?;
    }
    Ok(name)
}

/// Runs an instance's fetch now and stores the result. The error, if any, is returned too.
pub async fn fetch_instance(state: &SharedState, id: i64) -> anyhow::Result<Result<(), String>> {
    let Some(inst) = model::instance(&state.db.conn(), id)? else {
        anyhow::bail!("no app instance {id}");
    };
    let Some(app) = state.apps.get(&inst.app) else {
        return Ok(Err(format!("the app {:?} is not installed", inst.app)));
    };
    if !app.has_fetch {
        return Ok(Ok(()));
    }
    let config = inst.config.clone();
    let data_dir = state.cfg.data_dir.clone();
    let result = tokio::task::spawn_blocking(move || apps::fetch(&app, &config, &data_dir)).await?;
    let c = state.db.conn();
    if let Err(e) = &result {
        // Log a failure once, not on every retry.
        if inst.error.as_deref() != Some(e.as_str()) {
            model::log(&c, None, "warn", &format!("{} ({}): {e}", inst.name, inst.app), None)?;
        }
    }
    model::store_fetch(&c, id, &result)?;
    Ok(result.map(|_| ()))
}

/// Keeps every instance's data fresh, and clears out old images.
pub async fn scheduler(state: SharedState) {
    let mut tick = tokio::time::interval(Duration::from_secs(20));
    let mut last_prune = std::time::Instant::now() - Duration::from_secs(3600);
    loop {
        tick.tick().await;
        let due: Vec<i64> = match model::instances(&state.db.conn()) {
            Ok(list) => list
                .into_iter()
                .filter(|i| {
                    let Some(app) = state.apps.get(&i.app) else { return false };
                    if !app.has_fetch || app.error.is_some() {
                        return false;
                    }
                    // After a failure, try again sooner (but not more than once a minute).
                    let every = if i.error.is_some() { app.refresh_s.min(300) } else { app.refresh_s };
                    i.fetched_ms.is_none_or(|t| now_ms() - t >= every as i64 * 1000)
                })
                .map(|i| i.id)
                .collect(),
            Err(e) => {
                tracing::error!("listing app instances: {e}");
                continue;
            }
        };
        for id in due {
            if let Err(e) = fetch_instance(&state, id).await {
                tracing::error!("fetching instance {id}: {e:#}");
            }
        }
        if last_prune.elapsed() > Duration::from_secs(3600) {
            last_prune = std::time::Instant::now();
            prune_images(&state);
        }
    }
}

/// Images are only needed until the device has downloaded them; keep a day's worth.
fn prune_images(state: &SharedState) {
    let Ok(rd) = std::fs::read_dir(state.cfg.data_dir.join("images")) else { return };
    for e in rd.flatten() {
        let old = e.metadata().and_then(|m| m.modified()).map(|t| t.elapsed().unwrap_or_default() > Duration::from_secs(86400));
        if old.unwrap_or(false) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// One app drawn alone on a panel of the given size, for the app editor: its config is run,
/// then (if the app has one) its fetch, then its render. Nothing is stored.
pub async fn try_app(state: &SharedState, app: apps::AppMeta, config: String, w: u32, h: u32, size: &'static str, grays: u8) -> anyhow::Result<Vec<u8>> {
    let data_dir = state.cfg.data_dir.clone();
    tokio::task::spawn_blocking(move || {
        let p = match size {
            "full" => screen::preset("full"),
            "large" => screen::preset("two-thirds"),
            "half" => screen::preset("halves"),
            _ => screen::preset("quadrants"),
        };
        let title_bars: Vec<bool> = (0..p.slots.len()).map(|i| i == 0).collect();
        let geoms = screen::geometry(p, w, h, &title_bars);
        let g = geoms[0];
        let content = (|| {
            let data = if app.has_fetch { apps::fetch(&app, &config, &data_dir)? } else { serde_json::Value::Null };
            apps::render(&app, &config, &data, Slot { w: g.content.w, h: g.content.h, size: g.size, grays }, &data_dir)
        })();
        let mut slots = vec![SlotDraw { geom: g, content: Some(content), title: app.name.clone() }];
        slots.extend(geoms[1..].iter().map(|g| SlotDraw { geom: *g, content: None, title: String::new() }));
        screen::compose(w, h, grays, p.slots.len() > 1, &slots).to_device_png(crate::render::OG_IMAGE_LIMIT)
    })
    .await?
}
