use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let request = jnja_render::cli::parse_request()?;
    let started = Instant::now();
    let mut result = jnja_render::renderer::render(&request)?;
    result.render_time_ms = started.elapsed().as_millis() as u64;
    if let Some(path) = request.manifest_path() {
        jnja_render::output::write_json(&path, &result)?;
    }
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
