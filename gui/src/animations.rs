//! Built-in LED animations.
//!
//! The animations shipped with the app are plain JSON in `animations/` and
//! embedded at build time. They use the same [`Animation`] struct as
//! user-created animations (which live in the config), so a user can load a
//! built-in one, tweak it and save it under a new id.

use bloqsync::anim::Animation;

/// Every embedded animation file (one JSON file per animation in `animations/`).
/// The list is generated at build time by `build.rs`.
const FILES: &[&str] = include!(concat!(env!("OUT_DIR"), "/animations_index.rs"));

pub(crate) fn builtin() -> Vec<Animation> {
    FILES
        .iter()
        .filter_map(|raw| serde_json::from_str::<Animation>(raw).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bloqsync::anim::MOVEMENT_IDS;

    #[test]
    fn builtin_animations_are_valid_and_known() {
        let anims = builtin();
        assert!(anims.len() >= 12, "expected the full shipped set");
        let mut ids = std::collections::HashSet::new();
        for a in &anims {
            assert!(a.valid(), "{a:?}");
            assert!(ids.insert(a.id.clone()), "duplicate id {}", a.id);
            assert!(MOVEMENT_IDS.contains(&a.movement.as_str()), "unknown movement {a:?}");
        }
    }
}