use crate::{build_cli, output, renderer};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime},
};

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn site_arg(args: &[String]) -> PathBuf {
    let mut skip_value = false;
    for arg in args.iter().skip(1) {
        if skip_value {
            skip_value = false;
            continue;
        }
        if matches!(arg.as_str(), "--host" | "--port") {
            skip_value = true;
            continue;
        }
        if !arg.starts_with('-') {
            return PathBuf::from(arg);
        }
    }
    PathBuf::from(".")
}

fn ignored(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(
                "output"
                    | ".git"
                    | "node_modules"
                    | "target"
                    | ".cache"
                    | ".env"
                    | ".env.prod"
                    | ".env.test"
                    | "venv"
                    | ".venv"
                    | "__pycache__",
            )
        )
    })
}

fn snapshot(root: &Path) -> Result<BTreeMap<PathBuf, (u64, u128)>, Box<dyn std::error::Error>> {
    let mut files = BTreeMap::new();
    fn visit(
        root: &Path,
        path: &Path,
        files: &mut BTreeMap<PathBuf, (u64, u128)>,
    ) -> std::io::Result<()> {
        if ignored(path.strip_prefix(root).unwrap_or(path)) {
            return Ok(());
        }
        if path.is_dir() {
            for item in fs::read_dir(path)? {
                visit(root, &item?.path(), files)?;
            }
            return Ok(());
        }
        let metadata = fs::metadata(path)?;
        let modified = metadata
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH)
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        files.insert(
            path.strip_prefix(root).unwrap_or(path).to_path_buf(),
            (metadata.len(), modified),
        );
        Ok(())
    }
    visit(root, root, &mut files)?;
    Ok(files)
}

fn render(site: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let args = vec![
        "rinjarender".into(),
        site.to_string_lossy().into_owned(),
        "--incremental".into(),
    ];
    let request = build_cli::build_request(&args)?;
    let mut result = renderer::render(&request)?;
    result.render_time_ms = 0;
    if let Some(path) = request.manifest_path() {
        output::write_json(&path, &result)?;
    }
    println!(
        "[serve] rebuilt: changed={} removed={}",
        result.changed.len(),
        result.removed.len()
    );
    Ok(request.output_root)
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
    {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        _ => "application/octet-stream",
    }
}

fn decode_path(raw: &str) -> Option<PathBuf> {
    let path = raw.split('?').next()?.trim_start_matches('/');
    let path = if path.is_empty() {
        "index.html"
    } else if path.ends_with('/') {
        return decode_path(&format!("/{path}index.html"));
    } else if !path.contains('.') {
        return decode_path(&format!("/{path}/index.html"));
    } else {
        path
    };
    let relative = PathBuf::from(path);
    if relative
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        None
    } else {
        Some(relative)
    }
}

fn respond(mut stream: TcpStream, output_root: &Path) -> std::io::Result<()> {
    let mut request = [0_u8; 4096];
    let size = stream.read(&mut request)?;
    let line = String::from_utf8_lossy(&request[..size]);
    let target = line.split_whitespace().nth(1).and_then(decode_path);
    let (status, body, mime) = match target.map(|path| output_root.join(path)) {
        Some(path) if path.is_file() => ("200 OK", fs::read(&path)?, content_type(&path)),
        _ => (
            "404 Not Found",
            b"Not found\n".to_vec(),
            "text/plain; charset=utf-8",
        ),
    };
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())?;
    stream.write_all(&body)
}

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("rinjarender serve [SITE] [--host HOST] [--port PORT]");
        return Ok(());
    }
    let site = site_arg(args).canonicalize()?;
    let host = option(args, "--host").unwrap_or_else(|| "127.0.0.1".into());
    let port = option(args, "--port")
        .unwrap_or_else(|| "8000".into())
        .parse::<u16>()?;
    let output_root = render(&site)?;
    let listener = TcpListener::bind((host.as_str(), port))?;
    listener.set_nonblocking(true)?;
    println!("[serve] http://{host}:{port}/ -> {}", output_root.display());
    let mut previous = snapshot(&site)?;
    loop {
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    let _ = respond(stream, &output_root);
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error.into()),
            }
        }
        thread::sleep(Duration::from_millis(250));
        let current = snapshot(&site)?;
        if current != previous {
            match render(&site) {
                Ok(_) => {}
                Err(error) => eprintln!("[serve] rebuild failed; waiting for next change: {error}"),
            }
            previous = current;
        }
    }
}
