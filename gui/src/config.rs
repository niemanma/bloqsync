//! Persistent configuration: types, defaults and JSON persistence.
//!
//! The schema is append-only for compatibility: every field added after the
//! first release carries `#[serde(default)]`, so old config files keep loading.

use bloqsync::sampling::Layout;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Serialises read-modify-write access to the config file so that concurrent
/// command handlers (e.g. starting several bars at once) cannot overwrite each
/// other's changes.
static CONFIG_LOCK: Mutex<()> = Mutex::new(());

/// Stable device identity cached per USB port.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct Ident {
    pub(crate) uuid: String,
    pub(crate) leds: usize,
    pub(crate) firmware: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub(crate) struct BarConfig {
    pub(crate) bar_path: String,
    pub(crate) stream_index: usize,
    pub(crate) reverse: bool,
}

/// A saved mapping for one monitor setup (identified by `signature`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub(crate) struct Profile {
    pub(crate) signature: String,
    #[serde(default)]
    pub(crate) restore_token: Option<String>,
    #[serde(default)]
    pub(crate) bars: Vec<BarConfig>,
}

/// A user-named snapshot of the tunable global settings. Fields default so a
/// preset written by an older/newer UI version still loads.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct Preset {
    pub(crate) name: String,
    /// Bar → monitor assignment captured with the preset.
    pub(crate) bars: Vec<BarConfig>,
    pub(crate) fps: u32,
    pub(crate) smooth: f32,
    pub(crate) brightness: u8,
    pub(crate) left: usize,
    pub(crate) top: usize,
    pub(crate) right: usize,
    pub(crate) bottom: usize,
    pub(crate) filter: String,
    pub(crate) filter_strength: u8,
    pub(crate) max_frames: u32,
    pub(crate) custom_color: String,
    pub(crate) cinema_color: String,
    pub(crate) cinema_sensitivity: f32,
    pub(crate) cinema_brightness: f32,
    pub(crate) cinema_floor: f32,
    pub(crate) cinema_smooth: f32,
    pub(crate) cinema_contrast: f32,
    pub(crate) cinema_pulse: f32,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Config {
    pub(crate) bars: Vec<BarConfig>,
    pub(crate) fps: u32,
    pub(crate) smooth: f32,
    pub(crate) left: usize,
    pub(crate) top: usize,
    pub(crate) right: usize,
    pub(crate) bottom: usize,
    pub(crate) brightness: u8,
    #[serde(default = "default_filter")]
    pub(crate) filter: String,
    #[serde(default = "default_filter_strength")]
    pub(crate) filter_strength: u8,
    #[serde(default = "default_max_frames")]
    pub(crate) max_frames: u32,
    #[serde(default)]
    pub(crate) restore_token: Option<String>,
    #[serde(default)]
    pub(crate) autostart: bool,
    #[serde(default)]
    pub(crate) autostart_sync: bool,
    /// Per monitor-setup profiles (auto-created, auto-restored).
    #[serde(default)]
    pub(crate) profiles: Vec<Profile>,
    /// User-named setting presets.
    #[serde(default)]
    pub(crate) presets: Vec<Preset>,
    /// Persistent mapping USB-port id -> device UUID + info.
    #[serde(default)]
    pub(crate) bar_uuids: HashMap<String, Ident>,
    #[serde(default = "default_custom_color")]
    pub(crate) custom_color: String,
    #[serde(default = "default_cinema_color")]
    pub(crate) cinema_color: String,
    #[serde(default = "default_cin_sens")]
    pub(crate) cinema_sensitivity: f32,
    #[serde(default = "default_cin_bri")]
    pub(crate) cinema_brightness: f32,
    #[serde(default = "default_cin_floor")]
    pub(crate) cinema_floor: f32,
    #[serde(default = "default_cin_smooth")]
    pub(crate) cinema_smooth: f32,
    #[serde(default = "default_cin_contrast")]
    pub(crate) cinema_contrast: f32,
    #[serde(default)]
    pub(crate) cinema_pulse: f32,
}

fn default_custom_color() -> String {
    "#ff8800".to_string()
}
fn default_cinema_color() -> String {
    "#5a2882".to_string()
}
fn default_cin_sens() -> f32 {
    1.0
}
fn default_cin_bri() -> f32 {
    0.7
}
fn default_cin_floor() -> f32 {
    0.35
}
fn default_cin_smooth() -> f32 {
    0.6
}
fn default_cin_contrast() -> f32 {
    1.0
}

fn default_filter() -> String {
    "none".to_string()
}
fn default_filter_strength() -> u8 {
    8
}
fn default_max_frames() -> u32 {
    0
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bars: Vec::new(),
            fps: 24,
            smooth: 0.22,
            left: 18,
            top: 18,
            right: 18,
            bottom: 0,
            brightness: 200,
            filter: default_filter(),
            filter_strength: 8,
            max_frames: 0,
            restore_token: None,
            autostart: false,
            autostart_sync: false,
            profiles: Vec::new(),
            presets: Vec::new(),
            bar_uuids: HashMap::new(),
            custom_color: default_custom_color(),
            cinema_color: default_cinema_color(),
            cinema_sensitivity: 1.0,
            cinema_brightness: 0.7,
            cinema_floor: 0.35,
            cinema_smooth: 0.6,
            cinema_contrast: 1.0,
            cinema_pulse: 0.0,
        }
    }
}

impl Config {
    pub(crate) fn persist_uuids(&mut self, cache: &HashMap<String, Ident>) {
        for (id, ident) in cache {
            if !ident.uuid.is_empty() {
                self.bar_uuids.insert(id.clone(), ident.clone());
            }
        }
    }

    pub(crate) fn layout(&self) -> Layout {
        Layout {
            left: self.left,
            top: self.top,
            right: self.right,
            bottom: self.bottom,
        }
    }

    pub(crate) fn active_profile(&self, sig: &str) -> Option<&Profile> {
        if sig.is_empty() {
            return None;
        }
        self.profiles.iter().find(|p| p.signature == sig)
    }

    pub(crate) fn profile_mut(&mut self, sig: &str) -> &mut Profile {
        if !self.profiles.iter().any(|p| p.signature == sig) {
            self.profiles.push(Profile {
                signature: sig.to_string(),
                restore_token: None,
                bars: Vec::new(),
            });
        }
        self.profiles
            .iter_mut()
            .find(|p| p.signature == sig)
            .unwrap()
    }

    pub(crate) fn upsert_profile_token(&mut self, sig: &str, token: &str) {
        if sig.is_empty() {
            return;
        }
        self.profile_mut(sig).restore_token = Some(token.to_string());
    }

    pub(crate) fn upsert_profile_bar(&mut self, sig: &str, bar: BarConfig) {
        if sig.is_empty() {
            return;
        }
        let p = self.profile_mut(sig);
        if let Some(b) = p.bars.iter_mut().find(|b| b.bar_path == bar.bar_path) {
            *b = bar;
        } else {
            p.bars.push(bar);
        }
    }

    pub(crate) fn upsert_profile_bars(&mut self, sig: &str, bars: &[BarConfig]) {
        if sig.is_empty() {
            return;
        }
        self.profile_mut(sig).bars = bars.to_vec();
    }

    /// Insert a preset or replace the existing one with the same name.
    /// Presets without a name are ignored.
    pub(crate) fn upsert_preset(&mut self, preset: Preset) {
        if preset.name.trim().is_empty() {
            return;
        }
        match self.presets.iter_mut().find(|p| p.name == preset.name) {
            Some(existing) => *existing = preset,
            None => self.presets.push(preset),
        }
    }

    pub(crate) fn remove_preset(&mut self, name: &str) {
        self.presets.retain(|p| p.name != name);
    }

    /// Rename a preset. Returns `false` for a blank name, an unchanged name or
    /// a name that is already taken (so the caller can report it).
    pub(crate) fn rename_preset(&mut self, old: &str, new: &str) -> bool {
        let new = new.trim();
        if new.is_empty() || old == new {
            return false;
        }
        if self.presets.iter().any(|p| p.name == new) {
            return false;
        }
        match self.presets.iter_mut().find(|p| p.name == old) {
            Some(p) => {
                p.name = new.to_string();
                true
            }
            None => false,
        }
    }

    /// Fold a [`Config`] coming from the UI into `self` (the authoritative
    /// on-disk config): Rust-managed fields and bars the UI does not know about
    /// are preserved, and the resulting bar mapping is mirrored into the active
    /// profile so it can be restored later.
    pub(crate) fn merge_from_ui(&mut self, mut incoming: Config, sig: &str) {
        // The restore token is managed by Rust, not the UI.
        if incoming.restore_token.is_none() {
            incoming.restore_token = self.restore_token.clone();
        }
        // Profiles and presets are managed through their own commands.
        if incoming.profiles.is_empty() {
            incoming.profiles = self.profiles.clone();
        }
        if incoming.presets.is_empty() {
            incoming.presets = self.presets.clone();
        }
        // Keep bars that are not currently reported by the UI (e.g. a bar that
        // is temporarily unplugged or on another port).
        for b in self.bars.clone() {
            if !incoming.bars.iter().any(|n| n.bar_path == b.bar_path) {
                incoming.bars.push(b);
            }
        }
        let bars = incoming.bars.clone();
        incoming.upsert_profile_bars(sig, &bars);
        *self = incoming;
    }
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".config/bloqsync/config.json")
}

fn read_config_at(path: &Path) -> Config {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Write via a temp file + rename so a concurrent reader never sees a
/// half-written (unparseable) config.
fn write_config_at(path: &Path, cfg: &Config) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string_pretty(cfg) {
        let mut tmp = path.to_path_buf();
        tmp.set_extension("tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

pub(crate) fn read_config() -> Config {
    read_config_at(&config_path())
}

/// Read, mutate and write the config atomically with respect to every other
/// caller in this process. Use this for any read-modify-write sequence; plain
/// [`read_config`] is fine for read-only use.
pub(crate) fn update_config<R>(f: impl FnOnce(&mut Config) -> R) -> R {
    update_config_at(&config_path(), f)
}

fn update_config_at<R>(path: &Path, f: impl FnOnce(&mut Config) -> R) -> R {
    let _guard = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut cfg = read_config_at(path);
    let result = f(&mut cfg);
    write_config_at(path, &cfg);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(path: &str, stream: usize, reverse: bool) -> BarConfig {
        BarConfig { bar_path: path.into(), stream_index: stream, reverse }
    }

    #[test]
    fn layout_reflects_config_fields() {
        let mut cfg = Config::default();
        (cfg.left, cfg.top, cfg.right, cfg.bottom) = (1, 2, 3, 4);
        let l = cfg.layout();
        assert_eq!((l.left, l.top, l.right, l.bottom), (1, 2, 3, 4));
    }

    #[test]
    fn active_profile_requires_signature() {
        let mut cfg = Config::default();
        cfg.upsert_profile_bar("SIG", bar("1-2", 0, false));
        assert!(cfg.active_profile("SIG").is_some());
        assert!(cfg.active_profile("").is_none());
        assert!(cfg.active_profile("OTHER").is_none());
    }

    #[test]
    fn profile_mut_creates_only_once() {
        let mut cfg = Config::default();
        cfg.profile_mut("S");
        cfg.profile_mut("S");
        assert_eq!(cfg.profiles.len(), 1);
    }

    #[test]
    fn upsert_profile_token_ignores_empty_signature() {
        let mut cfg = Config::default();
        cfg.upsert_profile_token("", "tok");
        assert!(cfg.profiles.is_empty());
        cfg.upsert_profile_token("S", "tok");
        assert_eq!(cfg.active_profile("S").unwrap().restore_token.as_deref(), Some("tok"));
    }

    #[test]
    fn upsert_profile_bar_replaces_same_bar() {
        let mut cfg = Config::default();
        cfg.upsert_profile_bar("S", bar("p", 1, false));
        cfg.upsert_profile_bar("S", bar("p", 2, true));
        let p = cfg.active_profile("S").unwrap();
        assert_eq!(p.bars.len(), 1);
        assert_eq!(p.bars[0].stream_index, 2);
        assert!(p.bars[0].reverse);
    }

    #[test]
    fn upsert_profile_bars_replaces_whole_list() {
        let mut cfg = Config::default();
        cfg.upsert_profile_bars("S", &[bar("a", 0, false), bar("b", 1, false)]);
        cfg.upsert_profile_bars("S", &[bar("c", 2, true)]);
        let bars = &cfg.active_profile("S").unwrap().bars;
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].bar_path, "c");
        assert_eq!(bars[0].stream_index, 2);
        assert!(bars[0].reverse);
    }

    #[test]
    fn persist_uuids_skips_empty_uuids() {
        let mut cfg = Config::default();
        let mut cache = HashMap::new();
        cache.insert("1-2".to_string(), Ident { uuid: "abc".into(), leds: 54, firmware: "1.9.4".into() });
        cache.insert("1-3".to_string(), Ident::default());
        cfg.persist_uuids(&cache);
        assert!(cfg.bar_uuids.contains_key("1-2"));
        assert!(!cfg.bar_uuids.contains_key("1-3"));
    }

    #[test]
    fn legacy_config_deserializes_with_defaults() {
        // Only the fields written by early versions; everything newer must
        // fall back to its default so old config files keep loading.
        let json = r#"{"bars":[],"fps":30,"smooth":0.5,"left":1,"top":2,"right":3,"bottom":4,"brightness":180}"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.fps, 30);
        assert_eq!(cfg.filter, "none");
        assert_eq!(cfg.filter_strength, 8);
        assert_eq!(cfg.max_frames, 0);
        assert_eq!(cfg.custom_color, "#ff8800");
        assert_eq!(cfg.cinema_color, "#5a2882");
        assert!(!cfg.autostart);
        assert!(!cfg.autostart_sync);
        assert!(cfg.profiles.is_empty());
        assert!(cfg.bar_uuids.is_empty());
        assert_eq!(cfg.cinema_contrast, 1.0);
        assert_eq!(cfg.cinema_pulse, 0.0);
    }

    #[test]
    fn config_ignores_unknown_fields() {
        let json = r#"{"bars":[],"fps":24,"smooth":0.22,"left":18,"top":18,"right":18,"bottom":0,"brightness":200,"future":true}"#;
        assert!(serde_json::from_str::<Config>(json).is_ok());
    }

    fn preset(name: &str, brightness: u8) -> Preset {
        Preset { name: name.into(), brightness, ..Default::default() }
    }

    #[test]
    fn upsert_preset_inserts_then_replaces() {
        let mut cfg = Config::default();
        cfg.upsert_preset(preset("Wohnzimmer", 100));
        cfg.upsert_preset(preset("Kino", 50));
        cfg.upsert_preset(preset("Wohnzimmer", 200));
        assert_eq!(cfg.presets.len(), 2);
        let w = cfg.presets.iter().find(|p| p.name == "Wohnzimmer").unwrap();
        assert_eq!(w.brightness, 200);
    }

    #[test]
    fn upsert_preset_ignores_blank_name() {
        let mut cfg = Config::default();
        cfg.upsert_preset(preset("   ", 100));
        assert!(cfg.presets.is_empty());
    }

    #[test]
    fn remove_preset_drops_only_matching() {
        let mut cfg = Config::default();
        cfg.upsert_preset(preset("A", 1));
        cfg.upsert_preset(preset("B", 2));
        cfg.remove_preset("A");
        assert_eq!(cfg.presets.len(), 1);
        assert_eq!(cfg.presets[0].name, "B");
    }

    #[test]
    fn rename_preset_rules() {
        let mut cfg = Config::default();
        cfg.upsert_preset(preset("A", 1));
        cfg.upsert_preset(preset("B", 2));
        assert!(cfg.rename_preset("A", "C"));
        assert!(cfg.presets.iter().any(|p| p.name == "C"));
        // Blank, unchanged and duplicate names are rejected.
        assert!(!cfg.rename_preset("C", "  "));
        assert!(!cfg.rename_preset("C", "C"));
        assert!(!cfg.rename_preset("C", "B"));
        assert!(!cfg.rename_preset("missing", "D"));
    }

    #[test]
    fn preset_deserializes_partial_json() {
        // A container-level default lets older/newer presets load with the
        // remaining fields falling back to their defaults.
        let p: Preset = serde_json::from_str(r#"{"name":"Nur Name"}"#).unwrap();
        assert_eq!(p.name, "Nur Name");
        assert_eq!(p.fps, 0);
        assert_eq!(p.filter, "");
        assert!(p.bars.is_empty());
    }

    #[test]
    fn preset_round_trips_bar_assignment() {
        let p = Preset {
            name: "main2".into(),
            bars: vec![bar("a", 1, true), bar("b", 0, false)],
            ..Default::default()
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: Preset = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bars.len(), 2);
        assert_eq!(back.bars[0].bar_path, "a");
        assert_eq!(back.bars[0].stream_index, 1);
        assert!(back.bars[0].reverse);
    }

    #[test]
    fn legacy_config_has_no_presets() {
        let json = r#"{"bars":[],"fps":24,"smooth":0.22,"left":18,"top":18,"right":18,"bottom":0,"brightness":200}"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert!(cfg.presets.is_empty());
    }

    #[test]
    fn merge_from_ui_preserves_managed_fields() {
        let mut disk = Config::default();
        disk.restore_token = Some("tok".into());
        disk.upsert_preset(preset("Kino", 50));
        disk.bars = vec![bar("ghost", 2, false)];

        let mut ui = Config::default();
        ui.bars = vec![bar("a", 1, true), bar("b", 0, false)];

        disk.merge_from_ui(ui, "S");

        assert_eq!(disk.restore_token.as_deref(), Some("tok"));
        assert_eq!(disk.presets.len(), 1);
        // Bar the UI did not report is kept...
        assert!(disk.bars.iter().any(|b| b.bar_path == "ghost"));
        // ...and the active profile mirrors the full merged mapping.
        let prof = disk.active_profile("S").unwrap();
        assert_eq!(prof.bars.len(), 3);
        assert_eq!(prof.bars.iter().find(|b| b.bar_path == "a").unwrap().stream_index, 1);
    }

    #[test]
    fn merge_from_ui_uses_incoming_when_present() {
        let mut disk = Config::default();
        disk.restore_token = Some("old".into());
        disk.upsert_preset(preset("Keep", 1));

        let mut ui = Config::default();
        ui.restore_token = Some("new".into());
        ui.presets = vec![preset("OnlyNew", 2)];

        disk.merge_from_ui(ui, "S");
        assert_eq!(disk.restore_token.as_deref(), Some("new"));
        assert_eq!(disk.presets.len(), 1);
        assert_eq!(disk.presets[0].name, "OnlyNew");
    }

    #[test]
    fn concurrent_updates_do_not_lose_writes() {
        let dir = std::env::temp_dir().join(format!("bloqsync-cfg-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = std::sync::Arc::new(dir.join("config.json"));
        write_config_at(path.as_path(), &Config::default());

        let mut handles = Vec::new();
        for i in 0..8usize {
            let p = path.clone();
            handles.push(std::thread::spawn(move || {
                update_config_at(p.as_path(), |cfg| {
                    cfg.upsert_profile_bar("S", bar(&format!("bar{i}"), i, false));
                });
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let cfg = read_config_at(path.as_path());
        assert_eq!(cfg.active_profile("S").unwrap().bars.len(), 8);
        let _ = std::fs::remove_dir_all(&dir);
    }
}