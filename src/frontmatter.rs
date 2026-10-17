use crate::context::ContextMap;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Document {
    pub metadata: ContextMap,
    pub body: String,
}

pub fn split(source: &str) -> Result<Document, Box<dyn std::error::Error>> {
    if let Some((raw, body)) = split_yaml(source) {
        return Ok(Document {
            metadata: parse_yaml(raw)?,
            body,
        });
    }
    if let Some((metadata, body)) = split_legacy(source, '{', '}') {
        return Ok(Document { metadata, body });
    }
    if let Some((metadata, body)) = split_legacy(source, '_', '_') {
        return Ok(Document { metadata, body });
    }
    Ok(Document {
        metadata: Map::new(),
        body: source.to_owned(),
    })
}

fn split_yaml(source: &str) -> Option<(&str, String)> {
    let start = source
        .strip_prefix("---\n")
        .or_else(|| source.strip_prefix("---\r\n"))?;
    let end = start
        .find("\n---\n")
        .or_else(|| start.find("\r\n---\r\n"))?;
    let closing_len = if start.as_bytes().get(end) == Some(&b'\r') {
        7
    } else {
        5
    };
    Some((&start[..end], start[end + closing_len..].to_owned()))
}

fn split_legacy(source: &str, left: char, right: char) -> Option<(ContextMap, String)> {
    let lines: Vec<&str> = source.lines().collect();
    if lines.is_empty() {
        return None;
    }
    let mut header = Vec::new();
    let mut body_start = None;
    for (index, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            body_start = Some(index + 1);
            break;
        }
        let stripped = line.trim();
        let valid =
            stripped.starts_with(left) && stripped.ends_with(right) && stripped.contains(':');
        if valid {
            header.push(stripped);
        } else {
            return None;
        }
    }
    if header.is_empty() {
        return None;
    }
    let mut data = Map::new();
    for line in header {
        let inner = line.trim_start_matches(left).trim_end_matches(right).trim();
        let (key, value) = inner.split_once(':')?;
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        data.insert(key.to_owned(), coerce_legacy_value(value.trim()));
    }
    let body = lines[body_start.unwrap_or(lines.len())..].join("\n");
    Some((data, body))
}

fn parse_yaml(raw: &str) -> Result<ContextMap, Box<dyn std::error::Error>> {
    if raw.trim().is_empty() {
        return Ok(Map::new());
    }
    let values = yaml_serde::from_str::<BTreeMap<String, Value>>(raw)?;
    Ok(values.into_iter().collect())
}

fn coerce_legacy_value(value: &str) -> Value {
    let lower = value.to_ascii_lowercase();
    if matches!(lower.as_str(), "true" | "yes" | "on") {
        Value::Bool(true)
    } else if matches!(lower.as_str(), "false" | "no" | "off") {
        Value::Bool(false)
    } else if matches!(lower.as_str(), "null" | "none" | "~") {
        Value::Null
    } else if value.len() >= 2
        && (value.starts_with('\'') || value.starts_with('"'))
        && (value.ends_with('\'') || value.ends_with('"'))
    {
        Value::String(value[1..value.len() - 1].into())
    } else {
        Value::String(value.into())
    }
}

pub fn render_string_values<F>(
    metadata: &mut ContextMap,
    mut render: F,
) -> Result<(), Box<dyn std::error::Error>>
where
    F: FnMut(&str) -> Result<String, Box<dyn std::error::Error>>,
{
    fn visit<F>(value: &mut Value, render: &mut F) -> Result<(), Box<dyn std::error::Error>>
    where
        F: FnMut(&str) -> Result<String, Box<dyn std::error::Error>>,
    {
        match value {
            Value::String(text) => *text = render(text)?,
            Value::Array(values) => {
                for item in values {
                    visit(item, render)?;
                }
            }
            Value::Object(values) => {
                for item in values.values_mut() {
                    visit(item, render)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    for value in metadata.values_mut() {
        visit(value, &mut render)?;
    }
    Ok(())
}

/// Mirrors the small, observable alias set in markdown_renderer.py.
pub fn normalize_aliases(metadata: &mut ContextMap) {
    let aliases = [
        ("page_title", &["blog_title", "title"][..]),
        ("blog_title", &["page_title", "title"][..]),
        ("page_subtitle", &["subtitle"][..]),
        ("post_date", &["date"][..]),
        ("page_picture", &["blog_picture"][..]),
        ("blog_picture", &["page_picture"][..]),
    ];
    for (target, sources) in aliases {
        if metadata.contains_key(target) {
            continue;
        }
        for source in sources {
            if let Some(value) = metadata.get(*source).cloned() {
                metadata.insert(target.into(), value);
                break;
            }
        }
    }
    let underscore_values: Vec<(String, Value)> = metadata
        .iter()
        .filter_map(|(key, value)| {
            key.strip_prefix('_')
                .map(|name| (name.to_owned(), value.clone()))
        })
        .collect();
    for (key, value) in underscore_values {
        metadata.entry(key).or_insert(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_yaml() {
        let doc = split("---\ntitle: Hello\n---\nBody").unwrap();
        assert_eq!(doc.metadata["title"], "Hello");
        assert_eq!(doc.body, "Body");
    }
    #[test]
    fn recognizes_brace_header() {
        let doc = split("{title: Hello}\n{date: 2026}\n\nBody").unwrap();
        assert_eq!(doc.metadata["title"], "Hello");
    }
    #[test]
    fn recognizes_underscore_header() {
        let doc = split("_title: Hello_\n_date: 2026_\n\nBody").unwrap();
        assert_eq!(doc.metadata["title"], "Hello");
    }
    #[test]
    fn aliases_are_centralized() {
        let mut map = Map::new();
        map.insert("title".into(), Value::String("Hello".into()));
        normalize_aliases(&mut map);
        assert_eq!(map["page_title"], "Hello");
        assert_eq!(map["blog_title"], "Hello");
    }
}
