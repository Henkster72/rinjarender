use crate::request::RenderMode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactState {
    #[default]
    New,
    Changed,
    Unchanged,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileRecord {
    pub path: String,
    pub source: Option<String>,
    pub kind: String,
    pub bytes: u64,
    pub sha256: String,
    pub state: ArtifactState,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PageRecord {
    pub source: String,
    pub output: String,
    pub changed: bool,
    pub state: ArtifactState,
    #[serde(default)]
    pub fingerprint: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct StatePage {
    pub source: String,
    pub output: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct StaticFileState {
    pub source: String,
    pub output: String,
    pub size: u64,
    pub mtime_ns: u128,
    pub sha256: String,
    pub output_size: u64,
    pub output_mtime_ns: u128,
}

/// Persistent input/output state used to plan the next build.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildState {
    pub schema_version: u32,
    pub pages: BTreeMap<String, StatePage>,
    pub source_to_output: BTreeMap<String, String>,
    pub artifact_hashes: BTreeMap<String, String>,
    pub global_render_fingerprint: String,
    pub dependencies: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub static_files: BTreeMap<String, StaticFileState>,
}

impl BuildState {
    pub fn new() -> Self {
        Self {
            schema_version: 1,
            ..Self::default()
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderManifest {
    pub schema_version: u32,
    pub status: String,
    pub mode: RenderMode,
    pub generated: Vec<FileRecord>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    pub stale_candidates: Vec<String>,
    pub source_to_output: BTreeMap<String, String>,
    pub pages: Vec<PageRecord>,
    pub page_references: Vec<serde_json::Value>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    /// Diagnostic only; exclude from semantic output comparisons.
    pub render_time_ms: u64,
}
impl RenderManifest {
    pub fn new(mode: RenderMode) -> Self {
        Self {
            schema_version: 1,
            status: "ok".into(),
            mode,
            generated: vec![],
            changed: vec![],
            removed: vec![],
            stale_candidates: vec![],
            source_to_output: BTreeMap::new(),
            pages: vec![],
            page_references: vec![],
            warnings: vec![],
            errors: vec![],
            render_time_ms: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serializes_stable_contract() {
        let manifest = RenderManifest::new(RenderMode::Full);
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("source_to_output"));
        assert!(json.contains("render_time_ms"));
    }

    #[test]
    fn missing_static_state_deserializes_as_empty() {
        let value = serde_json::json!({
            "schema_version": 1,
            "pages": {},
            "source_to_output": {},
            "artifact_hashes": {},
            "global_render_fingerprint": "",
            "dependencies": {}
        });
        let state: BuildState = serde_json::from_value(value).unwrap();
        assert!(state.static_files.is_empty());
    }
}
