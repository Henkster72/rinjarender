use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ImageMetadata {
    pub path: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub mime: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageCacheEntry {
    pub path: String,
    pub mtime_ns: u64,
    pub size: u64,
    pub mime: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImageCache {
    pub entries: BTreeMap<String, ImageCacheEntry>,
}

impl ImageCache {
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(
            path,
            serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())
    }

    pub fn inspect(&mut self, path: &Path) -> Result<ImageMetadata, String> {
        let key = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .into_owned();
        let stat = fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mtime_ns = stat
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or_default();
        if let Some(entry) = self.entries.get(&key) {
            if entry.mtime_ns == mtime_ns && entry.size == stat.len() {
                return Ok(ImageMetadata {
                    path: entry.path.clone(),
                    width: entry.width,
                    height: entry.height,
                    mime: entry.mime.clone(),
                });
            }
        }
        let metadata = inspect(path)?;
        self.entries.insert(
            key,
            ImageCacheEntry {
                path: metadata.path.clone(),
                mtime_ns,
                size: stat.len(),
                mime: metadata.mime.clone(),
                width: metadata.width,
                height: metadata.height,
            },
        );
        Ok(metadata)
    }
}

pub fn mime_from_path(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "avif" => Some("image/avif"),
        "gif" => Some("image/gif"),
        "jpeg" | "jpg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "svg" => Some("image/svg+xml"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

pub fn inspect(path: &Path) -> Result<ImageMetadata, String> {
    let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let (width, height) = dimensions(path, &bytes);
    Ok(ImageMetadata {
        path: path.to_string_lossy().into_owned(),
        width,
        height,
        mime: mime_from_path(path).map(str::to_owned),
    })
}

pub fn inspect_with_cache(path: &Path, cache_path: &Path) -> Result<ImageMetadata, String> {
    if let Ok(text) = fs::read_to_string(cache_path) {
        if let Ok(cache) = serde_json::from_str::<Map<String, Value>>(&text) {
            if let Ok(stat) = fs::metadata(path) {
                let key = path
                    .canonicalize()
                    .unwrap_or_else(|_| path.to_path_buf())
                    .to_string_lossy()
                    .into_owned();
                if let Some(entry) = cache.get(&key) {
                    let mtime_matches = entry.get("mtime_ns").and_then(Value::as_u64)
                        == stat
                            .modified()
                            .ok()
                            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|duration| duration.as_nanos() as u64);
                    let size_matches =
                        entry.get("size").and_then(Value::as_u64) == Some(stat.len());
                    if mtime_matches && size_matches {
                        return Ok(ImageMetadata {
                            path: key,
                            width: entry.get("width").and_then(Value::as_u64).map(|v| v as u32),
                            height: entry
                                .get("height")
                                .and_then(Value::as_u64)
                                .map(|v| v as u32),
                            mime: entry
                                .get("mime_type")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                        });
                    }
                }
            }
        }
    }
    inspect(path)
}

pub fn resolve_local_image_path(site_root: &Path, image_ref: &str) -> Option<PathBuf> {
    let mut normalized = image_ref.trim().replace('\\', "/");
    if normalized.is_empty() || normalized.starts_with("data:") {
        return None;
    }
    if normalized.starts_with("//") {
        normalized = normalized
            .split_once('/')
            .map(|(_, value)| value.to_owned())?;
    } else if normalized.starts_with("http://") || normalized.starts_with("https://") {
        normalized = normalized.split_once("//")?.1.to_owned();
        normalized = normalized
            .split_once('/')
            .map(|(_, value)| value.to_owned())?;
    }
    let normalized = normalized.trim_start_matches('/');
    if normalized.is_empty() {
        return None;
    }
    let mut candidates = Vec::new();
    if normalized.starts_with("static/") {
        candidates.push(site_root.join(normalized));
        candidates.push(site_root.join("output").join(normalized));
    } else {
        if normalized.contains('/') {
            candidates.push(site_root.join("static").join(normalized));
            candidates.push(site_root.join("output/static").join(normalized));
        } else {
            candidates.push(site_root.join("static/images").join(normalized));
            candidates.push(site_root.join("output/static/images").join(normalized));
        }
        candidates.push(site_root.join("static").join(normalized));
        candidates.push(site_root.join("output/static").join(normalized));
        candidates.push(site_root.join(normalized));
        candidates.push(site_root.join("output").join(normalized));
    }
    candidates.into_iter().find(|path| path.is_file())
}

pub fn enrich_page_context(context: &mut Map<String, Value>, site_root: &Path) {
    let mut cache = ImageCache::default();
    enrich_page_context_cached(context, site_root, &mut cache);
}

pub fn enrich_page_context_cached(
    context: &mut Map<String, Value>,
    site_root: &Path,
    cache: &mut ImageCache,
) {
    let Some(page_picture) = context.get("page_picture").and_then(Value::as_str) else {
        return;
    };
    if page_picture.is_empty() {
        return;
    }
    let Some(path) = resolve_local_image_path(site_root, page_picture) else {
        return;
    };
    let Ok(metadata) = cache.inspect(&path) else {
        return;
    };
    context.insert("page_picture_path".into(), Value::String(metadata.path));
    context.insert(
        "page_picture_mime_type".into(),
        metadata.mime.map(Value::String).unwrap_or(Value::Null),
    );
    context.insert(
        "page_picture_width".into(),
        metadata.width.map(Value::from).unwrap_or(Value::Null),
    );
    context.insert(
        "page_picture_height".into(),
        metadata.height.map(Value::from).unwrap_or(Value::Null),
    );
}

fn dimensions(path: &Path, bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") => (
            Some(u32::from_be_bytes(bytes[16..20].try_into().unwrap())),
            Some(u32::from_be_bytes(bytes[20..24].try_into().unwrap())),
        ),
        "gif" if bytes.len() >= 10 && bytes.starts_with(b"GIF8") => (
            Some(u16::from_le_bytes([bytes[6], bytes[7]]) as u32),
            Some(u16::from_le_bytes([bytes[8], bytes[9]]) as u32),
        ),
        "jpg" | "jpeg" => jpeg_dimensions(bytes),
        "webp" => webp_dimensions(bytes),
        "avif" => ispe_dimensions(bytes),
        "svg" => svg_dimensions(bytes),
        _ => (None, None),
    }
}

fn jpeg_dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    if bytes.len() < 4 || &bytes[..2] != b"\xff\xd8" {
        return (None, None);
    }
    let mut index = 2;
    while index + 9 < bytes.len() {
        if bytes[index] != 0xff {
            index += 1;
            continue;
        }
        while index < bytes.len() && bytes[index] == 0xff {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        let marker = bytes[index];
        index += 1;
        if matches!(marker, 0xd8 | 0xd9 | 0x01) || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if index + 2 > bytes.len() {
            break;
        }
        let length = u16::from_be_bytes([bytes[index], bytes[index + 1]]) as usize;
        if length < 7 || index + length > bytes.len() {
            break;
        }
        if (0xc0..=0xc3).contains(&marker)
            || (0xc5..=0xc7).contains(&marker)
            || (0xc9..=0xcb).contains(&marker)
            || (0xcd..=0xcf).contains(&marker)
        {
            return (
                Some(u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]) as u32),
                Some(u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]) as u32),
            );
        }
        index += length;
    }
    (None, None)
}

fn webp_dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    if bytes.len() < 30 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return (None, None);
    }
    match &bytes[12..16] {
        b"VP8X" => (
            Some(1 + u24_le(&bytes[24..27])),
            Some(1 + u24_le(&bytes[27..30])),
        ),
        b"VP8 " => (
            Some(u16::from_le_bytes([bytes[26], bytes[27]]) as u32 & 0x3fff),
            Some(u16::from_le_bytes([bytes[28], bytes[29]]) as u32 & 0x3fff),
        ),
        _ => (None, None),
    }
}

fn ispe_dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    let Some(index) = bytes.windows(4).position(|window| window == b"ispe") else {
        return (None, None);
    };
    if index + 16 > bytes.len() {
        return (None, None);
    }
    (
        Some(u32::from_be_bytes(
            bytes[index + 8..index + 12].try_into().unwrap(),
        )),
        Some(u32::from_be_bytes(
            bytes[index + 12..index + 16].try_into().unwrap(),
        )),
    )
}

fn svg_dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    let source = String::from_utf8_lossy(bytes);
    let width = svg_attribute(&source, "width").and_then(parse_svg_dimension);
    let height = svg_attribute(&source, "height").and_then(parse_svg_dimension);
    if width.is_some() && height.is_some() {
        return (width, height);
    }
    let Some(view_box) = svg_attribute(&source, "viewBox") else {
        return (None, None);
    };
    let parts: Vec<_> = view_box
        .replace(',', " ")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if parts.len() == 4 {
        (parse_svg_number(&parts[2]), parse_svg_number(&parts[3]))
    } else {
        (None, None)
    }
}

fn svg_attribute(source: &str, name: &str) -> Option<String> {
    let start = source.find(&format!("{name}="))? + name.len() + 1;
    let quote = *source.as_bytes().get(start)? as char;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let end = source[start + 1..].find(quote)? + start + 1;
    Some(source[start + 1..end].to_owned())
}
fn parse_svg_dimension(value: String) -> Option<u32> {
    parse_svg_number(
        value
            .trim()
            .split(|character: char| !character.is_ascii_digit() && character != '.')
            .next()?,
    )
}
fn parse_svg_number(value: &str) -> Option<u32> {
    value.parse::<f64>().ok().map(|value| value.round() as u32)
}
fn u24_le(bytes: &[u8]) -> u32 {
    bytes[0] as u32 | ((bytes[1] as u32) << 8) | ((bytes[2] as u32) << 16)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_png_dimensions_and_mime() {
        let path = std::env::temp_dir().join("jnja-metadata-test.png");
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&640u32.to_be_bytes());
        bytes.extend_from_slice(&480u32.to_be_bytes());
        fs::write(&path, bytes).unwrap();
        let metadata = inspect(&path).unwrap();
        assert_eq!(metadata.width, Some(640));
        assert_eq!(metadata.height, Some(480));
        assert_eq!(metadata.mime.as_deref(), Some("image/png"));
        let _ = fs::remove_file(path);
    }
    #[test]
    fn reads_svg_viewbox_dimensions() {
        let path = std::env::temp_dir().join("jnja-metadata-test.svg");
        fs::write(&path, r#"<svg viewBox="0 0 320 180"></svg>"#).unwrap();
        let metadata = inspect(&path).unwrap();
        assert_eq!(metadata.width, Some(320));
        assert_eq!(metadata.height, Some(180));
        let _ = fs::remove_file(path);
    }
}
