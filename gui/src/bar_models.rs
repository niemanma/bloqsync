//! LED bar models ("Leisten-Modelle"): a bar's total LED count plus how those
//! LEDs are distributed over the screen edges (left / top / bottom / right).
//!
//! Built-in models are shipped as JSON in `models/` and embedded at build time.
//! User-defined models live in the config (see [`crate::config::Config`]).

use serde::{Deserialize, Serialize};

/// One selectable bar model.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct BarModel {
    pub(crate) id: String,
    pub(crate) name: String,
    /// Total number of LEDs on the bar.
    pub(crate) leds: usize,
    pub(crate) left: usize,
    pub(crate) top: usize,
    pub(crate) bottom: usize,
    pub(crate) right: usize,
}

impl BarModel {
    pub(crate) fn valid(&self) -> bool {
        !self.id.trim().is_empty() && !self.name.trim().is_empty() && self.leds > 0
    }
}

/// Models compiled into the application.
const BUILTIN: &[&str] = &[include_str!("../models/sync-24-54.json")];

pub(crate) fn builtin() -> Vec<BarModel> {
    BUILTIN
        .iter()
        .filter_map(|raw| serde_json::from_str(raw).ok())
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
        }
    }

    #[test]
    fn default_model_is_54_led_14_26_14() {
        let m = &builtin()[0];
        assert_eq!(m.leds, 54);
        assert_eq!((m.left, m.top, m.bottom, m.right), (14, 26, 0, 14));
    }
}