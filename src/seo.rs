use crate::{metadata, url};
use serde_json::{Map, Value};
use std::path::Path;

fn value_as_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        other => other.to_string(),
    }
}

fn context_value<'a>(context: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    context
        .get(key)
        .or_else(|| context.get(&key.to_ascii_lowercase()))
}

fn expand_string(template: &str, context: &Map<String, Value>) -> String {
    let mut result = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        result.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("}}") else {
            result.push_str(&rest[start..]);
            return result;
        };
        let key = after_start[..end].trim();
        if let Some(value) = context_value(context, key) {
            result.push_str(&value_as_string(value));
        }
        rest = &after_start[end + 2..];
    }
    result.push_str(rest);
    result
}

fn expand_value(value: &Value, context: &Map<String, Value>) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), expand_value(value, context)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| expand_value(value, context))
                .collect(),
        ),
        Value::String(value) => Value::String(expand_string(value, context)),
        other => other.clone(),
    }
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn page_picture_url(context: &Map<String, Value>) -> String {
    let picture = context_value(context, "page_picture")
        .map(value_as_string)
        .unwrap_or_default();
    if picture.is_empty() {
        return String::new();
    }
    if picture.starts_with("http://")
        || picture.starts_with("https://")
        || picture.starts_with("//")
        || picture.starts_with("data:")
    {
        return picture;
    }
    let base = context_value(context, "base_url")
        .map(value_as_string)
        .unwrap_or_default();
    if base.is_empty() {
        return format!("/static/images/{}", picture.trim_start_matches('/'));
    }
    format!(
        "{}/static/images/{}",
        base.trim_end_matches('/'),
        picture.trim_start_matches('/')
    )
}

fn inferred_image_mime(reference: &str) -> Option<&'static str> {
    let reference = reference
        .split_once('?')
        .map_or(reference, |(path, _)| path);
    let reference = reference
        .split_once('#')
        .map_or(reference, |(path, _)| path);
    metadata::mime_from_path(Path::new(reference))
}

fn add_derived_image_context(context: &mut Map<String, Value>) {
    let url = page_picture_url(context);
    if !url.is_empty() {
        context
            .entry("page_picture_url")
            .or_insert_with(|| Value::String(url.clone()));
        if url.starts_with("https://") {
            context
                .entry("page_picture_secure_url")
                .or_insert(Value::String(url));
        }
    }
    let mime_is_missing = context
        .get("page_picture_mime_type")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty);
    if mime_is_missing {
        let reference = context_value(context, "page_picture_url")
            .or_else(|| context_value(context, "page_picture"))
            .map(value_as_string)
            .unwrap_or_default();
        if let Some(mime) = inferred_image_mime(&reference) {
            context.insert("page_picture_mime_type".into(), Value::String(mime.into()));
        }
    }
}

fn breadcrumb_label(
    context: &Map<String, Value>,
    page_references: &[Value],
    sub_dir: &str,
    segment: &str,
) -> String {
    if let Some(title) = page_references.iter().find_map(|reference| {
        let reference = reference.as_object()?;
        let name = reference.get("page_name")?.as_str()?;
        let directory = reference
            .get("sub_dir")
            .and_then(Value::as_str)
            .unwrap_or("");
        (name == segment && directory == sub_dir)
            .then(|| reference.get("page_title").and_then(Value::as_str))
            .flatten()
            .filter(|title| {
                let site_title = context_value(context, "title_site")
                    .map(value_as_string)
                    .unwrap_or_default();
                !title.is_empty() && *title != site_title
            })
    }) {
        return title.to_owned();
    }
    segment
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn breadcrumb_json_ld(context: &Map<String, Value>, page_references: &[Value]) -> Option<Value> {
    let sub_dir = context.get("sub_dir")?.as_str()?;
    if sub_dir.is_empty() {
        return None;
    }
    let base_url = context.get("base_url")?.as_str()?;
    let current_url = context
        .get("current_url")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    if current_url.is_empty() {
        return None;
    }
    let mut items = vec![serde_json::json!({
        "@type": "ListItem",
        "position": 1,
        "name": "Home",
        "item": url::join_base_path(base_url, "/")
    })];
    let segments = sub_dir.split('/').filter(|segment| !segment.is_empty());
    let mut route = Vec::new();
    for (index, segment) in segments.enumerate() {
        route.push(segment);
        let directory = route[..route.len() - 1].join("/");
        items.push(serde_json::json!({
            "@type": "ListItem",
            "position": index + 2,
            "name": breadcrumb_label(context, page_references, &directory, segment),
            "item": url::join_base_path(base_url, &format!("/{}/", route.join("/")))
        }));
    }
    let page_title = context
        .get("page_title")
        .or_else(|| context.get("title"))
        .map(value_as_string)
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| {
            context
                .get("page_name")
                .map(value_as_string)
                .unwrap_or_default()
        });
    items.push(serde_json::json!({
        "@type": "ListItem",
        "position": items.len() + 1,
        "name": page_title,
        "item": current_url
    }));
    Some(serde_json::json!({
        "@type": "BreadcrumbList",
        "itemListElement": items
    }))
}

fn json_ld_graph(value: &mut Value) -> Option<&mut Vec<Value>> {
    let object = value.as_object_mut()?;
    if !object.contains_key("@graph") {
        let mut node = serde_json::Map::new();
        let keys = object
            .keys()
            .filter(|key| key.as_str() != "@context")
            .cloned()
            .collect::<Vec<_>>();
        for key in keys {
            if let Some(value) = object.remove(&key) {
                node.insert(key, value);
            }
        }
        if !node.is_empty() {
            object.insert("@graph".into(), Value::Array(vec![Value::Object(node)]));
        }
    }
    let graph = object
        .entry("@graph")
        .or_insert_with(|| Value::Array(Vec::new()));
    graph.as_array_mut()
}

fn canonical_fragment_url(base_url: &str, fragment: &str) -> String {
    format!("{}#{fragment}", url::join_base_path(base_url, "/"))
}

fn append_node(graph: &mut Vec<Value>, node: Value) {
    let node_id = node.get("@id").and_then(Value::as_str);
    if node_id.is_some_and(|id| {
        graph
            .iter()
            .any(|entry| entry.get("@id").and_then(Value::as_str) == Some(id))
    }) {
        return;
    }
    graph.push(node);
}

fn parse_schema_date(value: &str) -> Option<String> {
    let parts: Vec<_> = value
        .trim()
        .to_ascii_lowercase()
        .replace(['.', ','], "")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    if parts.len() != 3 {
        return None;
    }
    let (day, month, year) = if parts[0].parse::<u32>().is_ok() {
        (
            parts[0].parse::<u32>().ok()?,
            parts[1].as_str(),
            parts[2].parse::<i32>().ok()?,
        )
    } else {
        (
            parts[1].parse::<u32>().ok()?,
            parts[0].as_str(),
            parts[2].parse::<i32>().ok()?,
        )
    };
    let month = match month {
        "january" | "jan" | "januari" => 1,
        "february" | "feb" | "februari" => 2,
        "march" | "mar" | "maart" | "mrt" => 3,
        "april" | "apr" => 4,
        "may" | "mei" => 5,
        "june" | "jun" | "juni" => 6,
        "july" | "jul" | "juli" => 7,
        "august" | "aug" | "augustus" => 8,
        "september" | "sep" => 9,
        "october" | "oct" | "oktober" | "okt" => 10,
        "november" | "nov" => 11,
        "december" | "dec" => 12,
        _ => return None,
    };
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

fn add_page_schema(value: &mut Value, context: &Map<String, Value>, page_references: &[Value]) {
    let Some(base_url) = context
        .get("base_url")
        .map(value_as_string)
        .filter(|v| !v.is_empty())
    else {
        return;
    };
    let Some(current_url) = context
        .get("current_url")
        .map(value_as_string)
        .filter(|v| !v.is_empty())
    else {
        return;
    };
    let title = context
        .get("page_title")
        .or_else(|| context.get("title"))
        .map(value_as_string)
        .filter(|v| !v.is_empty())
        .unwrap_or_default();
    let description = context
        .get("page_subtitle")
        .or_else(|| context.get("meta_description"))
        .map(value_as_string)
        .filter(|v| !v.is_empty())
        .unwrap_or_default();
    let page_id = format!("{current_url}#webpage");
    let breadcrumb = breadcrumb_json_ld(context, page_references);
    let Some(graph) = json_ld_graph(value) else {
        return;
    };
    let website_id = canonical_fragment_url(&base_url, "website");
    let organization_id = canonical_fragment_url(&base_url, "organization");
    for node in graph.iter_mut() {
        if node.get("@type").and_then(Value::as_str) == Some("WebSite") {
            node.as_object_mut()
                .expect("JSON-LD node must be an object")
                .entry("@id")
                .or_insert_with(|| Value::String(website_id.clone()));
        }
    }
    if !graph
        .iter()
        .any(|node| node.get("@id").and_then(Value::as_str) == Some(organization_id.as_str()))
    {
        let mut organization = serde_json::json!({
            "@id": organization_id.clone(),
            "@type": "Organization",
            "name": context.get("title_site").map(value_as_string).unwrap_or_default(),
            "url": url::join_base_path(&base_url, "/")
        });
        if let Some(description) = context
            .get("meta_description")
            .map(value_as_string)
            .filter(|value| !value.is_empty())
        {
            organization["description"] = Value::String(description);
        }
        append_node(graph, organization);
    }
    if let Some(breadcrumb) = breadcrumb {
        let breadcrumb_id = format!("{current_url}#breadcrumb");
        let mut breadcrumb = breadcrumb;
        breadcrumb
            .as_object_mut()
            .unwrap()
            .insert("@id".into(), Value::String(breadcrumb_id.clone()));
        append_node(graph, breadcrumb);
        append_node(
            graph,
            serde_json::json!({
                "@id": page_id.clone(),
                "@type": "WebPage",
                "url": current_url,
                "name": title,
                "isPartOf": {"@id": website_id.clone()},
                "publisher": {"@id": organization_id.clone()},
                "description": description,
                "breadcrumb": {"@id": breadcrumb_id}
            }),
        );
    } else {
        let page_node = serde_json::json!({
            "@id": page_id.clone(),
            "@type": "WebPage",
            "url": current_url,
            "name": title,
            "isPartOf": {"@id": website_id.clone()},
            "publisher": {"@id": organization_id.clone()},
            "description": description
        });
        append_node(graph, page_node);
    }
    if context.get("sub_dir").and_then(Value::as_str) == Some("blog")
        && context
            .get("page_name")
            .and_then(Value::as_str)
            .is_some_and(|name| name != "index")
    {
        let mut node = serde_json::json!({
            "@id": format!("{current_url}#blogposting"),
            "@type": "BlogPosting",
            "headline": title,
            "mainEntityOfPage": {"@id": page_id},
            "publisher": {"@id": organization_id}
        });
        if let Some(date) = context
            .get("post_date")
            .or_else(|| context.get("date"))
            .and_then(Value::as_str)
            .and_then(parse_schema_date)
        {
            node["datePublished"] = Value::String(date);
        }
        if let Some(image) = context
            .get("page_picture_url")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        {
            node["image"] = Value::String(image.into());
        }
        append_node(graph, node);
    }
}

pub fn render_meta_tags(config: &Value, context: &mut Map<String, Value>) -> String {
    let page_references = context
        .get("page_references")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    render_meta_tags_with_references(config, context, &page_references)
}

pub fn render_meta_tags_with_references(
    config: &Value,
    context: &mut Map<String, Value>,
    page_references: &[Value],
) -> String {
    add_derived_image_context(context);
    let Some(seo) = config.get("seo").and_then(Value::as_object) else {
        return String::new();
    };
    let mut tags = Vec::new();
    if let Some(entries) = seo.get("meta_tags").and_then(Value::as_array) {
        for entry in entries {
            let Some(entry) = entry.as_object() else {
                continue;
            };
            let kind = entry
                .get("kind")
                .or_else(|| entry.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("name");
            let name = entry.get("name").and_then(Value::as_str).unwrap_or("");
            let raw = entry
                .get("value")
                .or_else(|| entry.get("content"))
                .map(value_as_string)
                .unwrap_or_default();
            let content = expand_string(&raw, context).trim().to_owned();
            if !name.is_empty() && !content.is_empty() {
                tags.push(format!(
                    "<meta {}=\"{}\" content=\"{}\">",
                    kind,
                    escape_attribute(name),
                    escape_attribute(&content)
                ));
                if kind == "property" && name == "og:image" {
                    for (key, attr) in [
                        ("page_picture_secure_url", "og:image:secure_url"),
                        ("page_picture_mime_type", "og:image:type"),
                        ("page_picture_width", "og:image:width"),
                        ("page_picture_height", "og:image:height"),
                    ] {
                        if let Some(value) = context.get(key).map(value_as_string) {
                            if !value.is_empty() {
                                tags.push(format!(
                                    "<meta property=\"{attr}\" content=\"{}\">",
                                    escape_attribute(&value)
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    let mut json_ld = seo.get("json_ld").map(|value| expand_value(value, context)).unwrap_or_else(|| {
        serde_json::json!({
            "@context": "https://schema.org",
            "@type": "WebSite",
            "url": context.get("base_url").map(value_as_string).unwrap_or_default(),
            "name": context.get("title_site").map(value_as_string).unwrap_or_default(),
            "description": context.get("meta_description").map(value_as_string).unwrap_or_default()
        })
    });
    add_page_schema(&mut json_ld, context, page_references);
    if let Ok(serialized) = serde_json::to_string(&json_ld) {
        tags.push(format!(
            "<script type=\"application/ld+json\">{serialized}</script>"
        ));
    }
    tags.join("\n")
}

pub fn json_ld_tags(meta_tags: &str) -> String {
    meta_tags
        .split("<script type=\"application/ld+json\">")
        .skip(1)
        .filter_map(|part| part.split_once("</script>").map(|(json, _)| json))
        .map(|json| format!("<script type=\"application/ld+json\">{json}</script>"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::render_meta_tags;
    use serde_json::json;

    #[test]
    fn renders_canonical_meta_and_json_ld_from_context() {
        let config = json!({
            "seo": {
                "meta_tags": [
                    {"kind": "name", "name": "description", "value": "{{ page_subtitle }}"},
                    {"kind": "property", "name": "og:url", "value": "{{ current_url }}"}
                ],
                "json_ld": {"@id": "{{ current_url }}#webpage", "description": "{{ page_subtitle }}"}
            }
        });
        let mut context = serde_json::Map::new();
        context.insert("page_subtitle".into(), json!("A & B"));
        context.insert("current_url".into(), json!("https://example.test/news/"));
        let tags = render_meta_tags(&config, &mut context);
        assert!(tags.contains("content=\"A &amp; B\""));
        assert!(tags.contains("https://example.test/news/#webpage"));
    }

    #[test]
    fn infers_remote_webp_mime_case_insensitively() {
        let config = json!({"seo": {"meta_tags": [
            {"kind": "property", "name": "og:image", "value": "{{ page_picture_url }}"}
        ]}});
        let mut context = serde_json::Map::new();
        context.insert(
            "page_picture".into(),
            json!("https://example.test/Image.WEBP"),
        );
        let tags = render_meta_tags(&config, &mut context);
        assert!(tags.contains("property=\"og:image:type\" content=\"image/webp\""));
    }

    #[test]
    fn preserves_local_mime_and_omits_unknown_mime() {
        let config = json!({"seo": {"meta_tags": [
            {"kind": "property", "name": "og:image", "value": "{{ page_picture_url }}"}
        ]}});
        let mut local = serde_json::Map::new();
        local.insert("page_picture".into(), json!("image.bin"));
        local.insert("page_picture_mime_type".into(), json!("image/png"));
        assert!(render_meta_tags(&config, &mut local).contains("content=\"image/png\""));
        let mut unknown = serde_json::Map::new();
        unknown.insert(
            "page_picture".into(),
            json!("https://example.test/image.bin"),
        );
        assert!(!render_meta_tags(&config, &mut unknown).contains("og:image:type"));
    }

    #[test]
    fn adds_nested_breadcrumb_list_with_normalized_urls() {
        let config = json!({"seo": {"json_ld": {"@context": "https://schema.org", "@graph": []}}});
        let mut context = serde_json::Map::new();
        context.insert("base_url".into(), json!("https://example.test/"));
        context.insert("sub_dir".into(), json!("nieuws"));
        context.insert("page_name".into(), json!("item"));
        context.insert("page_title".into(), json!("Item"));
        context.insert(
            "current_url".into(),
            json!("https://example.test/nieuws/item/"),
        );
        context.insert(
            "page_references".into(),
            json!([{
                "page_name": "nieuws", "sub_dir": "", "page_title": "Nieuws"
            }]),
        );
        let tags = render_meta_tags(&config, &mut context);
        assert!(tags.contains("BreadcrumbList"));
        assert!(tags.contains("https://example.test/nieuws/"));
        assert!(tags.contains("https://example.test/nieuws/item/"));
        assert!(tags.contains("\"position\":3"));
    }

    #[test]
    fn omits_breadcrumb_for_root_and_top_level_pages() {
        let config = json!({"seo": {"json_ld": {"@graph": []}}});
        for (sub_dir, current_url) in [
            ("", "https://example.test/"),
            ("", "https://example.test/about/"),
        ] {
            let mut context = serde_json::Map::new();
            context.insert("base_url".into(), json!("https://example.test"));
            context.insert("sub_dir".into(), json!(sub_dir));
            context.insert("current_url".into(), json!(current_url));
            assert!(!render_meta_tags(&config, &mut context).contains("BreadcrumbList"));
        }
    }

    #[test]
    fn emits_default_schema_for_v2_without_explicit_json_ld() {
        let config = json!({"seo": {"meta_tags": []}});
        let mut context = serde_json::Map::new();
        context.insert("base_url".into(), json!("https://example.test/"));
        context.insert("title_site".into(), json!("Example"));
        context.insert("meta_description".into(), json!("Description"));
        context.insert(
            "current_url".into(),
            json!("https://example.test/blog/item/"),
        );
        context.insert("sub_dir".into(), json!("blog"));
        context.insert("page_name".into(), json!("item"));
        context.insert("page_title".into(), json!("Item"));
        context.insert("page_subtitle".into(), json!("Item description"));
        context.insert("post_date".into(), json!("sep 10, 2024"));
        let tags = render_meta_tags(&config, &mut context);
        assert!(tags.contains("WebSite"));
        assert!(tags.contains("Organization"));
        assert!(tags.contains(r#""@id":"https://example.test/#website""#));
        assert!(tags.contains("BreadcrumbList"));
        assert!(tags.contains("BlogPosting"));
        assert!(tags.contains("2024-09-10"));
    }

    #[test]
    fn breadcrumb_uses_section_title_when_available() {
        let config = json!({"seo": {"json_ld": {"@graph": []}}});
        let mut context = serde_json::Map::new();
        context.insert("base_url".into(), json!("https://example.test/"));
        context.insert("title_site".into(), json!("Example site"));
        context.insert("sub_dir".into(), json!("blog"));
        context.insert("page_name".into(), json!("article"));
        context.insert("page_title".into(), json!("Article title"));
        context.insert(
            "current_url".into(),
            json!("https://example.test/blog/article/"),
        );
        context.insert(
            "page_references".into(),
            json!([{"page_name": "blog", "sub_dir": "", "page_title": "Example site"}]),
        );
        let tags = render_meta_tags(&config, &mut context);
        let json = tags
            .split_once("<script type=\"application/ld+json\">")
            .and_then(|(_, rest)| rest.split_once("</script>"))
            .map(|(json, _)| serde_json::from_str::<serde_json::Value>(json).unwrap())
            .unwrap();
        let breadcrumb = json["@graph"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["@type"] == "BreadcrumbList")
            .unwrap();
        assert_eq!(breadcrumb["itemListElement"][1]["name"], "Blog");
    }
}
