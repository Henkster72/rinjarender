use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderMode {
    #[default]
    Full,
    Incremental,
    Selected,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderEnvironment {
    #[default]
    Production,
    Development,
}

/// Rendering input only. Credentials, uploads and notifications stay outside.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderRequest {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub site_root: PathBuf,
    #[serde(default)]
    pub config: Option<PathBuf>,
    pub output_root: PathBuf,
    #[serde(default)]
    pub mode: RenderMode,
    #[serde(default)]
    pub templates: Vec<String>,
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub context: Option<PathBuf>,
    #[serde(default)]
    pub environment: RenderEnvironment,
    #[serde(default)]
    pub manifest: Option<PathBuf>,
    #[serde(default)]
    pub state: Option<PathBuf>,
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
    #[serde(default)]
    pub prune: bool,
    #[serde(default)]
    pub verify_static: bool,
    #[serde(default)]
    pub render_only: bool,
}
fn default_schema_version() -> u32 {
    1
}

impl RenderRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err(format!(
                "unsupported request schema {}",
                self.schema_version
            ));
        }
        if self.site_root.as_os_str().is_empty() || self.output_root.as_os_str().is_empty() {
            return Err("site_root and output_root are required".into());
        }
        if matches!(self.mode, RenderMode::Selected) && self.templates.is_empty() {
            return Err("selected mode requires at least one template".into());
        }
        if self
            .templates
            .iter()
            .any(|p| Path::new(p).is_absolute() || p.split('/').any(|part| part == ".."))
        {
            return Err("templates must be relative paths below site_root".into());
        }
        if let Some(layout) = self.layout.as_deref() {
            if Path::new(layout).is_absolute() || layout.split('/').any(|part| part == "..") {
                return Err("layout must be a relative template path".into());
            }
        }
        Ok(())
    }
    pub fn config_path(&self) -> PathBuf {
        self.config
            .as_ref()
            .map(|p| {
                if p.is_absolute() {
                    p.clone()
                } else {
                    self.site_root.join(p)
                }
            })
            .unwrap_or_else(|| self.site_root.join("site_config.json"))
    }
    pub fn context_path(&self) -> Option<PathBuf> {
        self.context.as_ref().map(|p| {
            if p.is_absolute() {
                p.clone()
            } else {
                self.site_root.join(p)
            }
        })
    }
    pub fn manifest_path(&self) -> Option<PathBuf> {
        self.manifest.as_ref().map(|p| {
            if p.is_absolute() {
                p.clone()
            } else {
                self.site_root.join(p)
            }
        })
    }
    pub fn state_path(&self) -> Option<PathBuf> {
        self.state.as_ref().map(|p| {
            if p.is_absolute() {
                p.clone()
            } else {
                self.site_root.join(p)
            }
        })
    }
    pub fn image_cache_path(&self) -> PathBuf {
        self.cache_dir
            .as_ref()
            .map(|p| {
                if p.is_absolute() {
                    p.clone()
                } else {
                    self.site_root.join(p)
                }
            })
            .unwrap_or_else(|| self.output_root.join(".jnja-cache"))
            .join("image-metadata-cache.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> RenderRequest {
        RenderRequest {
            schema_version: 1,
            site_root: "/site".into(),
            config: None,
            output_root: "/out".into(),
            mode: RenderMode::Selected,
            templates: vec!["index.html".into()],
            layout: None,
            context: Some("ctx.json".into()),
            environment: RenderEnvironment::Production,
            manifest: None,
            state: None,
            cache_dir: None,
            prune: false,
            verify_static: false,
            render_only: false,
        }
    }
    #[test]
    fn defaults_are_safe() {
        assert_eq!(RenderMode::default(), RenderMode::Full);
        assert_eq!(RenderEnvironment::default(), RenderEnvironment::Production);
    }
    #[test]
    fn paths_resolve_under_site_root() {
        let r = request();
        assert_eq!(r.config_path(), PathBuf::from("/site/site_config.json"));
        assert_eq!(r.context_path().unwrap(), PathBuf::from("/site/ctx.json"));
    }
    #[test]
    fn traversal_is_rejected() {
        let mut r = request();
        r.templates = vec!["../secret.html".into()];
        assert!(r.validate().is_err());
    }
    #[test]
    fn selected_requires_sources() {
        let mut r = request();
        r.templates.clear();
        assert!(r.validate().is_err());
    }
}
