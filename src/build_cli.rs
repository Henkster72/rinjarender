use crate::{
    config, output, renderer,
    request::{RenderEnvironment, RenderMode, RenderRequest},
};
use serde_json::Value;
use std::{
    env, io,
    path::{Path, PathBuf},
    time::Instant,
};

fn optional(args: &[String], flag: &str) -> Option<PathBuf> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .map(PathBuf::from)
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn site_argument(args: &[String]) -> Result<(PathBuf, bool), String> {
    if let Some(site) = optional(args, "--site-root").or_else(|| optional(args, "--site")) {
        return Ok((site, false));
    }
    let value_flags = [
        "--config",
        "--context",
        "--manifest",
        "--state",
        "--cache-dir",
        "--output",
        "--output-root",
        "--state-dir",
    ];
    let mut skip_value = false;
    for arg in args.iter().skip(1) {
        if skip_value {
            skip_value = false;
            continue;
        }
        if value_flags.contains(&arg.as_str()) {
            skip_value = true;
            continue;
        }
        if !arg.starts_with('-') {
            return Ok((PathBuf::from(arg), true));
        }
    }
    Ok((env::current_dir().map_err(|error| error.to_string())?, true))
}

fn relative_or_absolute(root: &Path, value: PathBuf) -> PathBuf {
    if value.is_absolute() {
        value
    } else {
        root.join(value)
    }
}

fn configured_output(site_root: &Path, config: &Value) -> PathBuf {
    let output = config
        .pointer("/paths/output")
        .and_then(Value::as_str)
        .or_else(|| config.get("output").and_then(Value::as_str))
        .unwrap_or("output");
    relative_or_absolute(site_root, PathBuf::from(output))
}

fn default_state_dir(site_root: &Path) -> Result<PathBuf, String> {
    let cache_root = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or_else(|| "XDG_CACHE_HOME or HOME is required for local build state".to_owned())?;
    let identity = site_root.file_name().ok_or_else(|| {
        format!(
            "cannot derive a local state name from {}",
            site_root.display()
        )
    })?;
    Ok(cache_root.join("jnja/local").join(identity))
}

pub(crate) fn build_request(args: &[String]) -> Result<RenderRequest, Box<dyn std::error::Error>> {
    let (site_root, direct_cli) = site_argument(args)?;
    let site_root = site_root
        .canonicalize()
        .map_err(|error| format!("site root unavailable: {error}"))?;
    let config_path = optional(args, "--config")
        .map(|path| relative_or_absolute(&site_root, path))
        .unwrap_or_else(|| site_root.join("site_config.json"));
    if !config_path.is_file() {
        return Err(format!("site_config.json not found: {}", config_path.display()).into());
    }
    let site_config = config::load(&site_root, &config_path)
        .map_err(|error| format!("could not load {}: {error}", config_path.display()))?;
    let state_dir = optional(args, "--state-dir")
        .map(|path| relative_or_absolute(&site_root, path))
        .map(Ok)
        .unwrap_or_else(|| default_state_dir(&site_root))?;
    let output_root = optional(args, "--output")
        .or_else(|| optional(args, "--output-root"))
        .map(|path| relative_or_absolute(&site_root, path))
        .unwrap_or_else(|| configured_output(&site_root, &site_config));
    let incremental = has_flag(args, "--incremental") || (direct_cli && !has_flag(args, "--full"));
    if incremental && has_flag(args, "--full") {
        return Err("--full and --incremental cannot be used together".into());
    }
    let state = optional(args, "--state").unwrap_or_else(|| state_dir.join("state.json"));
    let manifest = optional(args, "--manifest").unwrap_or_else(|| state_dir.join("result.json"));
    let cache_dir = optional(args, "--cache-dir").unwrap_or_else(|| state_dir.join("cache"));
    Ok(RenderRequest {
        schema_version: 1,
        site_root,
        config: Some(config_path),
        output_root,
        mode: if incremental {
            RenderMode::Incremental
        } else {
            RenderMode::Full
        },
        templates: vec![],
        layout: None,
        context: optional(args, "--context"),
        environment: RenderEnvironment::Production,
        manifest: Some(manifest),
        state: Some(state),
        cache_dir: Some(cache_dir),
        prune: false,
        verify_static: has_flag(args, "--verify-static"),
        render_only: has_flag(args, "--render-only"),
    })
}

fn program_name(args: &[String]) -> &str {
    args.first()
        .and_then(|arg| Path::new(arg).file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("opensite-render")
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.get(1).map(String::as_str) == Some("init") {
        return crate::init_cli::run(&args[2..]);
    }
    if args.get(1).map(String::as_str) == Some("serve") {
        return crate::serve_cli::run(&args[1..]);
    }
    let name = program_name(&args);
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{name} [SITE] [--site SITE] [--output DIR] [--state-dir DIR] [--full|--incremental] [--render-only]");
        println!("Legacy automation flags remain supported: --site-root, --output-root, --state, --manifest and --cache-dir.");
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{name} {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let request = build_request(&args)
        .map_err(|error| format!("[config] invalid site configuration\nreason: {error}"))?;
    request
        .validate()
        .map_err(|error| format!("[config] invalid render request\nreason: {error}"))?;
    let cancelled = crate::interrupt::install()?;
    let started = Instant::now();
    let mut result = renderer::render(&request)
        .map_err(|error| format!("[render] template/content rendering failed\nreason: {error}"))?;
    if crate::interrupt::requested(&cancelled) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "render cancelled").into());
    }
    result.render_time_ms = started.elapsed().as_millis() as u64;
    if let Some(path) = request.manifest_path() {
        output::write_json(&path, &result).map_err(|error| {
            format!(
                "[output] could not write build result\npath: {}\nreason: {error}",
                path.display()
            )
        })?;
    }
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn direct_site_defaults_to_local_incremental_state_and_configured_output() {
        let root = env::temp_dir().join(format!("jnja-cli-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("site_config.json"),
            r#"{"schema_version":2,"paths":{"output":"public"}}"#,
        )
        .unwrap();
        let request =
            build_request(&["jnja-build".into(), root.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(request.mode, RenderMode::Incremental);
        assert_eq!(request.output_root, root.join("public"));
        let state_dir = request.state.as_ref().unwrap().parent().unwrap();
        assert_eq!(state_dir.file_name(), root.file_name());
        assert_eq!(
            state_dir.parent().unwrap().file_name(),
            Some(std::ffi::OsStr::new("local"))
        );
        assert_eq!(request.cache_dir, Some(state_dir.join("cache")));
    }

    #[test]
    fn full_flag_and_explicit_paths_are_supported() {
        let root = env::temp_dir().join(format!("jnja-cli-full-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("site_config.json"), r#"{"schema_version":2}"#).unwrap();
        let request = build_request(&[
            "jnja-build".into(),
            root.to_string_lossy().into_owned(),
            "--full".into(),
            "--output".into(),
            "build".into(),
            "--state-dir".into(),
            ".local-state".into(),
        ])
        .unwrap();
        assert_eq!(request.mode, RenderMode::Full);
        assert_eq!(request.output_root, root.join("build"));
        assert_eq!(request.state, Some(root.join(".local-state/state.json")));
    }
}
