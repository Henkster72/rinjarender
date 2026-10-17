use crate::{config, request::RenderRequest};
use serde_json::Value;
use std::path::PathBuf;

pub struct Site {
    pub root: PathBuf,
    pub templates: PathBuf,
    pub config: Value,
}
pub fn load(request: &RenderRequest) -> Result<Site, Box<dyn std::error::Error>> {
    let config_path = request.config_path();
    let config: Value = config::load(&request.site_root, &config_path)?;
    Ok(Site {
        root: request.site_root.clone(),
        templates: request.site_root.join("templates"),
        config,
    })
}
pub fn input_path(site: &Site, relative: &str) -> PathBuf {
    let relative = relative.strip_prefix("templates/").unwrap_or(relative);
    site.templates.join(relative)
}
