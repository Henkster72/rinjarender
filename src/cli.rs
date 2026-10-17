use crate::request::{RenderEnvironment, RenderMode, RenderRequest};
use std::{env, fs, path::PathBuf};

pub fn parse_request() -> Result<RenderRequest, Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("jnja-render --request request.json\n  or: --site PATH --input REL --output PATH [--template NAME]");
        std::process::exit(0);
    }
    if let Some(i) = args.iter().position(|a| a == "--request") {
        let path = args.get(i + 1).ok_or("missing value for --request")?;
        let request: RenderRequest = serde_json::from_str(&fs::read_to_string(path)?)?;
        request.validate()?;
        return Ok(request);
    }
    let value = |flag: &str, default: Option<&str>| -> Result<String, String> {
        match args.iter().position(|a| a == flag) {
            Some(i) => args
                .get(i + 1)
                .cloned()
                .ok_or_else(|| format!("missing value for {flag}")),
            None => default
                .map(str::to_owned)
                .ok_or_else(|| format!("missing required flag {flag}")),
        }
    };
    let request = RenderRequest {
        schema_version: 1,
        site_root: PathBuf::from(value("--site", None)?),
        config: Some(PathBuf::from(value("--config", Some("site_config.json"))?)),
        output_root: PathBuf::from(value("--output", None)?),
        mode: RenderMode::Selected,
        templates: vec![value("--input", None)?],
        layout: Some(value("--template", Some("layouts/markdown_page.html"))?),
        context: value("--context", None).ok().map(PathBuf::from),
        environment: RenderEnvironment::Production,
        manifest: value("--manifest", None).ok().map(PathBuf::from),
        state: value("--state", None).ok().map(PathBuf::from),
        cache_dir: value("--cache-dir", None).ok().map(PathBuf::from),
        prune: false,
        verify_static: args.iter().any(|arg| arg == "--verify-static"),
        render_only: args.iter().any(|arg| arg == "--render-only"),
    };
    request.validate()?;
    Ok(request)
}
