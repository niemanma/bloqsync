//! LED bar models ("Leisten-Modelle"): everything needed to identify and drive
//! one kind of bar — how it is recognised on the USB bus, its LED count, how the
//! LEDs are distributed over the screen edges, and the wire protocol it speaks.
//!
//! Built-in models are shipped as one JSON file each in `models/` and embedded
//! at build time (see `build.rs`). User-defined models live in the config (see
//! [`crate::config::Config`]). The point of this layout is that a new model can
//! be added — or shared through a future "store" — by dropping in a JSON file.

use serde::{Deserialize, Serialize};

/// How a bar is recognised on the USB bus. `*_id`/`usage_page` are hex strings
/// (e.g. `"1a86"`), so a model file stays readable.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct ModelMatch {
    pub(crate) vendor_id: String,
    pub(crate) product_id: String,
    pub(crate) usage_page: String,
}

impl Default for ModelMatch {
    fn default() -> Self {
        ModelMatch {
            vendor_id: "1a86".into(),
            product_id: "fe07".into(),
            usage_page: "ff00".into(),
        }
    }
}

impl ModelMatch {
    fn hex(s: &str) -> Option<u16> {
        u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
    }
    pub(crate) fn vendor(&self) -> Option<u16> { Self::hex(&self.vendor_id) }
    pub(crate) fn product(&self) -> Option<u16> { Self::hex(&self.product_id) }
    pub(crate) fn usage(&self) -> Option<u16> { Self::hex(&self.usage_page) }
}

/// The wire protocol a model speaks. Mirrors the constants in the core
/// `bloqsync::protocol` / `bloqsync::device` modules so a future alternative
/// bar can describe its own framing here.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct ModelProtocol {
    pub(crate) kind: String,
    pub(crate) report_size: usize,
    pub(crate) max_sections_per_frame: usize,
    pub(crate) section_size: usize,
    pub(crate) max_leds: usize,
    pub(crate) inter_frame_gap_ms: u64,
    pub(crate) actions: ModelActions,
}

impl Default for ModelProtocol {
    fn default() -> Self {
        ModelProtocol {
            kind: "robobloq-synclight".into(),
            report_size: 64,
            max_sections_per_frame: 11,
            section_size: 5,
            max_leds: 254,
            inter_frame_gap_ms: 3,
            actions: ModelActions::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct ModelActions {
    pub(crate) sync_screen: String,
    pub(crate) section_led: String,
    pub(crate) brightness: String,
    pub(crate) device_info: String,
}

impl Default for ModelActions {
    fn default() -> Self {
        ModelActions {
            sync_screen: "0x80".into(),
            section_led: "0x86".into(),
            brightness: "0x87".into(),
            device_info: "0x82".into(),
        }
    }
}

fn hex_u8(s: &str) -> Option<u8> {
    u8::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok()
}

/// One selectable bar model.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct BarModel {
    pub(crate) id: String,
    pub(crate) name: String,
    /// How the bar is identified on the USB bus.
    #[serde(rename = "match")]
    pub(crate) match_: ModelMatch,
    /// Total number of LEDs on the bar.
    pub(crate) leds: usize,
    pub(crate) left: usize,
    pub(crate) top: usize,
    pub(crate) bottom: usize,
    pub(crate) right: usize,
    /// Wire protocol used to drive the bar.
    pub(crate) protocol: ModelProtocol,
}

impl Default for BarModel {
    fn default() -> Self {
        BarModel {
            id: String::new(),
            name: String::new(),
            match_: ModelMatch::default(),
            leds: 0,
            left: 0,
            top: 0,
            bottom: 0,
            right: 0,
            protocol: ModelProtocol::default(),
        }
    }
}

impl BarModel {
    pub(crate) fn valid(&self) -> bool {
        !self.id.trim().is_empty()
            && !self.name.trim().is_empty()
            && self.leds > 0
            // The edge distribution must exactly cover the LED count, otherwise
            // part of the strip stays dark (and the sampler is misaligned).
            && self.left + self.top + self.bottom + self.right == self.leds
    }
}

/// Every embedded model file (one JSON file per model in `models/`).
const FILES: &[&str] = include!(concat!(env!("OUT_DIR"), "/models_index.rs"));

pub(crate) fn builtin() -> Vec<BarModel> {
    FILES
        .iter()
        .filter_map(|raw| serde_json::from_str(raw).ok())
        .collect()
}

/// Identification rules derived from the built-in **and** user-defined models,
/// used by the core to recognise bars on the USB bus and to speak their
/// protocol. Called at start-up and again whenever a model is added/removed.
pub(crate) fn device_match_rules() -> Vec<bloqsync::device::DeviceMatch> {
    let mut models = builtin();
    models.extend(crate::config::read_config().bar_models);
    models
        .into_iter()
        .filter_map(|m| {
            let def = bloqsync::protocol::ProtocolSpec::default();
            let p = &m.protocol;
            Some(bloqsync::device::DeviceMatch {
                vendor_id: m.match_.vendor()?,
                product_id: m.match_.product()?,
                usage_page: m.match_.usage()?,
                protocol: bloqsync::protocol::ProtocolSpec {
                    report_size: p.report_size,
                    max_sections_per_frame: p.max_sections_per_frame,
                    section_size: p.section_size,
                    max_leds: p.max_leds,
                    inter_frame_gap_ms: p.inter_frame_gap_ms,
                    act_sync_screen: hex_u8(&p.actions.sync_screen).unwrap_or(def.act_sync_screen),
                    act_section_led: hex_u8(&p.actions.section_led).unwrap_or(def.act_section_led),
                    act_brightness: hex_u8(&p.actions.brightness).unwrap_or(def.act_brightness),
                    act_read_info: hex_u8(&p.actions.device_info).unwrap_or(def.act_read_info),
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_models_are_valid() {
        let models = builtin();
        assert!(!models.is_empty());
        for m in &models {
            assert!(m.valid(), "{m:?}");
            // The shipped default distributes exactly its LED count.
            assert_eq!(m.left + m.top + m.bottom + m.right, m.leds, "{m:?}");
            assert!(m.match_.vendor().is_some(), "{m:?}");
            assert!(m.match_.product().is_some(), "{m:?}");
            assert!(m.match_.usage().is_some(), "{m:?}");
        }
    }

    #[test]
    fn default_model_is_54_led_14_26_14() {
        let m = &builtin()[0];
        assert_eq!(m.leds, 54);
        assert_eq!((m.left, m.top, m.bottom, m.right), (14, 26, 0, 14));
    }
}
