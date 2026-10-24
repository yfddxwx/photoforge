//! photoforge-presets
//!
//! JSON preset files and `.pfg` sidecars (the non-destructive edit state
//! stored next to the original image — ADR-0010).
//!
//! Presets capture only the *global* adjustment stack (parameters + enabled
//! flags) — never masks, geometry or crops, which are image-specific.

use photoforge_ops::params::GLOBAL_OP_ORDER;
use photoforge_ops::OpParams;
use photoforge_pipeline::EditState;
use serde::{Deserialize, Serialize};

/// A saved set of global adjustments.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    /// Format version (currently 1).
    pub version: u32,
    pub name: String,
    /// One entry per global op, in [`GLOBAL_OP_ORDER`].
    pub adjustments: Vec<PresetAdjustment>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PresetAdjustment {
    pub op: OpParams,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("version mismatch: file is v{file}, this build writes v{expected}")]
    Version { file: u32, expected: u32 },
}

pub const FORMAT_VERSION: u32 = 1;

impl Preset {
    /// Captures the global stack from an edit state.
    pub fn from_state(state: &EditState, name: &str) -> Self {
        let adjustments = state
            .adjustments
            .iter()
            .take(GLOBAL_OP_ORDER.len())
            .map(|a| PresetAdjustment {
                op: a.op.clone(),
                enabled: a.enabled,
            })
            .collect();
        Self {
            version: FORMAT_VERSION,
            name: name.to_string(),
            adjustments,
        }
    }

    /// Applies the preset onto an edit state's global stack (leaves local
    /// adjustments and geometry untouched).
    pub fn apply_to(&self, state: &mut EditState) {
        for (i, pa) in self.adjustments.iter().enumerate() {
            if let Some(adj) = state.adjustments.get_mut(i) {
                adj.op = pa.op.clone();
                adj.enabled = pa.enabled;
            }
        }
    }

    pub fn to_json(&self) -> Result<String, PresetError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn from_json(text: &str) -> Result<Self, PresetError> {
        let p: Preset = serde_json::from_str(text)?;
        if p.version != FORMAT_VERSION {
            return Err(PresetError::Version {
                file: p.version,
                expected: FORMAT_VERSION,
            });
        }
        Ok(p)
    }

    pub fn from_file(path: &std::path::Path) -> Result<Self, PresetError> {
        Self::from_json(&std::fs::read_to_string(path)?)
    }

    pub fn to_file(&self, path: &std::path::Path) -> Result<(), PresetError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_json()?)?;
        Ok(())
    }
}

/// The sidecar path for an image: `<full original file name>.pfg`
/// (`IMG_0001.JPG` → `IMG_0001.JPG.pfg`, unambiguous for multiple formats).
pub fn sidecar_path_for(image: &std::path::Path) -> std::path::PathBuf {
    let mut name = image
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".pfg");
    image.with_file_name(name)
}

/// Writes the edit state as a `.pfg` sidecar next to the image.
pub fn save_sidecar(
    image: &std::path::Path,
    state: &EditState,
) -> Result<std::path::PathBuf, PresetError> {
    let path = sidecar_path_for(image);
    let doc = serde_json::json!({
        "format": "photoforge-sidecar",
        "version": FORMAT_VERSION,
        "state": state,
    });
    std::fs::write(&path, serde_json::to_string_pretty(&doc)?)?;
    Ok(path)
}

/// Loads the edit state from a sidecar, if one exists.
pub fn load_sidecar(image: &std::path::Path) -> Result<Option<EditState>, PresetError> {
    let path = sidecar_path_for(image);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    let doc: serde_json::Value = serde_json::from_str(&text)?;
    let state: EditState = serde_json::from_value(doc.get("state").cloned().unwrap_or_default())?;
    Ok(Some(state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use photoforge_pipeline::CropRect;

    #[test]
    fn preset_roundtrip_and_apply() {
        let mut state = EditState::with_global_stack();
        state.adjustments[1].op = OpParams::Exposure { stops: 0.7 };
        state.adjustments[2].op = OpParams::Contrast { amount: 0.25 };
        state.adjustments[2].enabled = false;
        state.geometry.crop = Some(CropRect {
            x: 1,
            y: 2,
            w: 3,
            h: 4,
        });
        let preset = Preset::from_state(&state, "My look");
        let json = preset.to_json().expect("serialize");
        let back = Preset::from_json(&json).expect("deserialize");
        assert_eq!(back, preset);
        assert_eq!(back.adjustments.len(), GLOBAL_OP_ORDER.len());
        assert!(!json.contains("crop"), "presets must not carry geometry");

        // Apply onto a different state.
        let mut fresh = EditState::with_global_stack();
        back.apply_to(&mut fresh);
        assert_eq!(fresh.adjustments[1].op, OpParams::Exposure { stops: 0.7 });
        assert!(!fresh.adjustments[2].enabled);
        assert!(fresh.geometry.crop.is_none(), "geometry untouched");
    }

    #[test]
    fn preset_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!("pfg_preset_{}", std::process::id()));
        let mut state = EditState::with_global_stack();
        state.adjustments[11].op = OpParams::Saturation { amount: -0.4 };
        let preset = Preset::from_state(&state, "film");
        let path = dir.join("film.json");
        preset.to_file(&path).expect("write");
        let back = Preset::from_file(&path).expect("read");
        assert_eq!(back.name, "film");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn version_mismatch_is_an_error() {
        let bad = r#"{"version": 99, "name": "x", "adjustments": []}"#;
        assert!(matches!(
            Preset::from_json(bad),
            Err(PresetError::Version { .. })
        ));
    }

    #[test]
    fn sidecar_roundtrip() {
        let dir = std::env::temp_dir().join(format!("pfg_sidecar_{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let image = dir.join("IMG_0001.JPG");
        let mut state = EditState::with_global_stack();
        state.adjustments[1].op = OpParams::Exposure { stops: 1.0 };
        state.geometry.straighten_deg = -2.5;
        let saved = save_sidecar(&image, &state).expect("save");
        assert_eq!(saved, dir.join("IMG_0001.JPG.pfg"));
        let loaded = load_sidecar(&image).expect("load").expect("exists");
        assert_eq!(loaded, state);
        // No sidecar → None.
        assert!(load_sidecar(&dir.join("other.png"))
            .expect("load")
            .is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
