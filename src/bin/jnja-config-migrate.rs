use jnja_render::{config, output};
use std::{env, path::PathBuf};

fn value(args: &[String], flag: &str) -> Result<PathBuf, String> {
    let index = args
        .iter()
        .position(|arg| arg == flag)
        .ok_or_else(|| format!("missing {flag}"))?;
    args.get(index + 1)
        .map(PathBuf::from)
        .ok_or_else(|| format!("missing value for {flag}"))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("jnja-config-migrate --site-root SITE --output CONFIG");
        return Ok(());
    }
    let site_root = value(&args, "--site-root")?;
    let output_path = value(&args, "--output")?;
    let value = config::migrate(&site_root, &site_root.join("site_config.json"))?;
    output::write_json(&output_path, &value)?;
    println!("wrote {}", output_path.display());
    Ok(())
}
