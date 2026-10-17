use crate::{context::ContextMap, discovery::SourceEntry, frontmatter, metadata, source, url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PageNode {
    pub source: String,
    pub kind: source::SourceType,
    pub output: String,
    pub route: String,
    pub depth: i64,
    pub page_name: String,
    pub sub_dir: String,
    pub metadata: ContextMap,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SiteGraph {
    pub pages: Vec<PageNode>,
}

/// Immutable indexes shared by every page render in one build.
pub struct RenderIndex<'a> {
    pub pages_by_source: HashMap<&'a str, &'a PageNode>,
    pub logical_pages: Vec<&'a PageNode>,
    pub cards_by_section: HashMap<String, Vec<&'a PageNode>>,
    pub page_references: Vec<Value>,
    pub page_references_value: minijinja::value::Value,
}

impl SiteGraph {
    pub fn discover(
        entries: &[SourceEntry],
        site_root: &Path,
        config: &Value,
        base_url: Option<&str>,
        image_cache: &mut metadata::ImageCache,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut pages = Vec::with_capacity(entries.len());
        for entry in entries {
            let output = source::source_to_output_path(Path::new(&entry.relative_path))
                .ok_or_else(|| format!("cannot map {}", entry.relative_path))?;
            let output = output.to_string_lossy().replace('\\', "/");
            let route = url::route_for_output(&output);
            let depth = output.matches('/').count() as i64;
            let (page_name, sub_dir) = route_parts(&route);
            let raw = fs::read_to_string(&entry.absolute_path)?;
            let mut metadata = if entry.kind == source::SourceType::Markdown {
                frontmatter::split(&raw)?.metadata
            } else {
                html_metadata(&raw)
            };
            frontmatter::normalize_aliases(&mut metadata);
            let defaults = config.get("defaults").cloned().unwrap_or(Value::Null);
            if !metadata.contains_key("page_picture") {
                if let Some(value) = defaults.get("page_picture") {
                    metadata.insert("page_picture".into(), value.clone());
                }
            }
            if !metadata.contains_key("page_card_template") {
                if let Some(value) = section_card_template(config, &sub_dir)
                    .or_else(|| defaults.get("page_card_template"))
                {
                    metadata.insert("page_card_template".into(), value.clone());
                }
            }
            if !metadata.contains_key("page_title") {
                if let Some(value) = defaults.get("page_title") {
                    metadata.insert("page_title".into(), value.clone());
                }
            }
            if let Some(picture) = metadata
                .get("page_picture")
                .and_then(Value::as_str)
                .map(str::to_owned)
            {
                let mut image_found = false;
                if let Some(path) = metadata::resolve_local_image_path(site_root, &picture) {
                    if let Ok(image) = image_cache.inspect(&path) {
                        image_found = true;
                        metadata.insert("page_picture_path".into(), Value::String(image.path));
                        metadata.insert(
                            "page_picture_mime_type".into(),
                            image.mime.map(Value::String).unwrap_or(Value::Null),
                        );
                        metadata.insert(
                            "page_picture_width".into(),
                            image.width.map(Value::from).unwrap_or(Value::Null),
                        );
                        metadata.insert(
                            "page_picture_height".into(),
                            image.height.map(Value::from).unwrap_or(Value::Null),
                        );
                    }
                }
                let _ = image_found;
                if let Some(base) = base_url {
                    metadata.insert(
                        "page_picture_url".into(),
                        Value::String(format!(
                            "{}/static/images/{}",
                            base.trim_end_matches('/'),
                            picture
                        )),
                    );
                }
            }
            let fingerprint = page_fingerprint(&raw, &metadata);
            pages.push(PageNode {
                source: entry.relative_path.clone(),
                kind: entry.kind,
                output,
                route,
                depth,
                page_name,
                sub_dir,
                metadata,
                fingerprint,
            });
        }
        pages.sort_by(|a, b| a.source.cmp(&b.source));
        Ok(Self { pages })
    }

    pub fn logical_pages(&self) -> Vec<&PageNode> {
        let mut selected: std::collections::BTreeMap<&str, &PageNode> =
            std::collections::BTreeMap::new();
        for page in &self.pages {
            selected
                .entry(&page.output)
                .and_modify(|current| {
                    if precedence(page) > precedence(current) {
                        *current = page;
                    }
                })
                .or_insert(page);
        }
        selected.into_values().collect()
    }

    pub fn page_references(&self) -> Vec<Value> {
        self.pages.iter().map(|page| {
            let m = &page.metadata;
            serde_json::json!({
                "rel_path": page.source, "page_name": page.page_name, "sub_dir": page.sub_dir,
                "page_title": m.get("page_title").or_else(|| m.get("title")).cloned().unwrap_or_else(|| Value::String(title_from_slug(&page.page_name))),
                "page_subtitle": m.get("page_subtitle").or_else(|| m.get("subtitle")).cloned().unwrap_or_else(|| Value::String(String::new())),
                "page_picture": m.get("page_picture").or_else(|| m.get("blog_picture")).cloned().unwrap_or_else(|| Value::String(String::new())),
                "page_icon": m.get("page_icon").cloned().unwrap_or_else(|| Value::String(String::new())),
                "page_picture_url": m.get("page_picture_url").cloned().unwrap_or_else(|| Value::String(String::new())),
                "page_picture_mime_type": m.get("page_picture_mime_type").cloned().unwrap_or(Value::Null),
                "page_picture_width": m.get("page_picture_width").cloned().unwrap_or(Value::Null),
                "page_picture_height": m.get("page_picture_height").cloned().unwrap_or(Value::Null),
                "post_date": m.get("post_date").or_else(|| m.get("date")).cloned().unwrap_or_else(|| Value::String(String::new())),
                "cta": m.get("cta").cloned().unwrap_or(Value::Bool(false)), "testimonial_name": m.get("testimonial_name").cloned().unwrap_or_else(|| Value::String(String::new())),
                "testimonial_location": m.get("testimonial_location").cloned().unwrap_or_else(|| Value::String(String::new())), "audio_source": m.get("audio_source").cloned().unwrap_or_else(|| Value::String(String::new())),
                "card_ratio": m.get("card_ratio").cloned().unwrap_or_else(|| Value::String(String::new())), "external_url": m.get("external_url").cloned().unwrap_or_else(|| Value::String(String::new())),
                "external_target": m.get("external_target").cloned().unwrap_or_else(|| Value::String("_blank".into())), "card_only": m.get("card_only").cloned().unwrap_or(Value::Bool(false)),
                "page_card_template": m.get("page_card_template").cloned().unwrap_or_else(|| Value::String(String::new()))
            })
        }).collect()
    }

    pub fn render_index(&self) -> RenderIndex<'_> {
        let logical_pages = self.logical_pages();
        let mut cards_by_section: HashMap<String, Vec<&PageNode>> = HashMap::new();
        for page in &logical_pages {
            cards_by_section
                .entry(page.sub_dir.clone())
                .or_default()
                .push(page);
        }
        for pages in cards_by_section.values_mut() {
            pages.sort_by_key(card_sort_key_ref);
            pages.reverse();
        }
        let page_references = self.page_references();
        let page_references_value = minijinja::value::Value::from_serialize(&page_references);
        RenderIndex {
            pages_by_source: self
                .pages
                .iter()
                .map(|page| (page.source.as_str(), page))
                .collect(),
            logical_pages,
            cards_by_section,
            page_references,
            page_references_value,
        }
    }

    pub fn card_pages(&self, sub_dir: &str) -> Vec<&PageNode> {
        let mut pages: Vec<_> = self
            .logical_pages()
            .into_iter()
            .filter(|page| page.sub_dir == sub_dir)
            .collect();
        pages.sort_by_key(card_sort_key_ref);
        pages.reverse();
        pages
    }
}

fn section_card_template<'a>(config: &'a Value, sub_dir: &str) -> Option<&'a Value> {
    config
        .get("card_templates")
        .and_then(Value::as_object)
        .and_then(|templates| templates.get(sub_dir))
}

fn card_sort_key(page: &PageNode) -> (i32, i32, i32, i32, String) {
    let date = page
        .metadata
        .get("post_date")
        .or_else(|| page.metadata.get("date"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let parts: Vec<_> = date
        .replace(['.', ','], "")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let month = match parts.first().map(String::as_str) {
        Some("january" | "jan" | "januari") => 1,
        Some("february" | "feb" | "februari") => 2,
        Some("march" | "mar" | "maart" | "mrt") => 3,
        Some("april" | "apr") => 4,
        Some("may" | "mei") => 5,
        Some("june" | "jun" | "juni") => 6,
        Some("july" | "jul" | "juli") => 7,
        Some("august" | "aug" | "augustus") => 8,
        Some("september" | "sep") => 9,
        Some("october" | "oct" | "oktober" | "okt") => 10,
        Some("november" | "nov") => 11,
        Some("december" | "dec") => 12,
        _ => 0,
    };
    let day = parts
        .get(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let year = parts
        .get(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    (year, month, day, 0, page.source.clone())
}

fn card_sort_key_ref(page: &&PageNode) -> (i32, i32, i32, i32, String) {
    card_sort_key(page)
}

fn precedence(page: &PageNode) -> (u8, u8, &str) {
    // Python's discovery loop renders both counterparts; the Markdown
    // counterpart is the effective winner when they map to one output path.
    let kind = if page.kind == source::SourceType::Markdown {
        2
    } else {
        1
    };
    let root = u8::from(page.source.starts_with("root/"));
    (kind, root, &page.source)
}

fn route_parts(route: &str) -> (String, String) {
    let clean = route.trim_matches('/');
    if clean.is_empty() {
        return ("index".into(), String::new());
    }
    let mut parts = clean.rsplitn(2, '/');
    let name = parts.next().unwrap_or("index").to_owned();
    (name, parts.next().unwrap_or("").to_owned())
}
fn title_from_slug(value: &str) -> String {
    if value == "index" {
        value.into()
    } else {
        value
            .replace(['-', '_'], " ")
            .split_whitespace()
            .map(|x| {
                let mut c = x.chars();
                match c.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}
fn sha256(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn page_fingerprint(source: &str, metadata: &ContextMap) -> String {
    let mut bytes = source.as_bytes().to_vec();
    let mut stable_metadata = metadata.clone();
    // Absolute cache/source paths vary between a production tree and a shadow
    // copy, but the image dimensions and MIME are effective page inputs.
    stable_metadata.remove("page_picture_path");
    if let Ok(metadata) = serde_json::to_vec(&stable_metadata) {
        bytes.extend_from_slice(&metadata);
    }
    sha256(&bytes)
}
fn html_metadata(source: &str) -> ContextMap {
    let mut result = ContextMap::new();
    for (key, pattern) in [
        ("page_title", "page_title"),
        ("page_subtitle", "page_subtitle"),
        ("page_picture", "page_picture"),
        ("page_icon", "page_icon"),
        ("post_date", "post_date"),
        ("external_url", "external_url"),
        ("page_card_template", "page_card_template"),
        ("card_ratio", "card_ratio"),
        ("audio_source", "audio_source"),
        ("testimonial_name", "testimonial_name"),
        ("testimonial_location", "testimonial_location"),
    ] {
        let marker = format!("{{% set {pattern} = \"");
        if let Some(start) = source.find(&marker) {
            let rest = &source[start + marker.len()..];
            if let Some(end) = rest.find("\" %}") {
                result.insert(key.into(), Value::String(rest[..end].into()));
            }
        }
    }
    for key in [
        "cta",
        "card_only",
        "blog",
        "sociallinks",
        "faq",
        "bigfooter",
    ] {
        let marker = format!("{{% set {key} = ");
        if let Some(start) = source.find(&marker) {
            let value = source[start + marker.len()..].trim_start();
            if value.starts_with("True") || value.starts_with("true") {
                result.insert(key.into(), Value::Bool(true));
            } else if value.starts_with("False") || value.starts_with("false") {
                result.insert(key.into(), Value::Bool(false));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routes_use_complete_path() {
        assert_eq!(url::route_for_output("index.html"), "/");
        assert_eq!(url::route_for_output("blog/post/index.html"), "/blog/post/");
        assert_eq!(route_parts("/deep/a/b/"), ("b".into(), "deep/a".into()));
    }

    #[test]
    fn markdown_wins_over_html_and_root_wins_same_kind() {
        let node = |source: &str, kind: source::SourceType| PageNode {
            source: source.into(),
            kind,
            output: "x/index.html".into(),
            route: "/x/".into(),
            depth: 1,
            page_name: "x".into(),
            sub_dir: String::new(),
            metadata: ContextMap::new(),
            fingerprint: source.into(),
        };
        let graph = SiteGraph {
            pages: vec![
                node("x.md", source::SourceType::Markdown),
                node("x.html", source::SourceType::Html),
                node("root/x.md", source::SourceType::Markdown),
            ],
        };
        let selected = graph.logical_pages();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].source, "root/x.md");
    }

    #[test]
    fn section_card_template_precedes_global_default() {
        let config = serde_json::json!({
            "card_templates": {"nieuws": "nieuws_card.htm"},
            "defaults": {"page_card_template": "generic_card.htm"}
        });
        assert_eq!(
            section_card_template(&config, "nieuws").and_then(Value::as_str),
            Some("nieuws_card.htm")
        );
        assert_eq!(section_card_template(&config, "partners"), None);
    }

    #[test]
    fn card_sort_key_supports_english_month_abbreviations() {
        let node = |source: &str, date: &str| PageNode {
            source: source.into(),
            kind: source::SourceType::Html,
            output: format!("blog/{source}/index.html"),
            route: format!("/blog/{source}/"),
            depth: 2,
            page_name: source.into(),
            sub_dir: "blog".into(),
            metadata: serde_json::json!({"post_date": date})
                .as_object()
                .cloned()
                .unwrap(),
            fingerprint: source.into(),
        };
        let pages = vec![node("old", "dec 17, 2019"), node("new", "sep 10, 2024")];
        assert!(card_sort_key(&pages[1]) > card_sort_key(&pages[0]));
    }

    #[test]
    fn render_index_precomputes_shared_pages_and_card_candidates() {
        let node = |source: &str, sub_dir: &str| PageNode {
            source: source.into(),
            kind: source::SourceType::Html,
            output: format!("{source}/index.html"),
            route: format!("/{source}/"),
            depth: 1,
            page_name: source.into(),
            sub_dir: sub_dir.into(),
            metadata: ContextMap::new(),
            fingerprint: source.into(),
        };
        let graph = SiteGraph {
            pages: vec![node("one", "blog"), node("two", "blog"), node("about", "")],
        };
        let index = graph.render_index();

        assert_eq!(index.logical_pages.len(), 3);
        assert_eq!(index.pages_by_source.get("two").unwrap().page_name, "two");
        assert_eq!(index.cards_by_section.get("blog").unwrap().len(), 2);
        assert_eq!(
            index.page_references_value,
            index.page_references_value.clone()
        );
    }

    #[test]
    fn html_metadata_extracts_audio_source() {
        let metadata = html_metadata(r#"{% set audio_source = "audio/test.m4a" %}"#);
        assert_eq!(metadata["audio_source"], "audio/test.m4a");
    }
}
