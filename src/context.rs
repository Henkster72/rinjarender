use crate::{config, url};
use minijinja::value::Value;
use serde_json::{Map, Value as JsonValue};
use std::{fs, path::Path};
pub type ContextMap = Map<String, JsonValue>;
pub fn load_json(path: Option<&Path>) -> Result<ContextMap, Box<dyn std::error::Error>> {
    let Some(path) = path else {
        return Ok(Map::new());
    };
    let value: JsonValue = serde_json::from_str(&fs::read_to_string(path)?)?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| format!("context must be a JSON object: {}", path.display()).into())
}
pub fn as_template_value(context: &ContextMap) -> Value {
    Value::from_serialize(context)
}

/// Combines the mutable page context with build-shared page references without
/// serializing the reference collection again.
pub fn as_template_value_with_references(context: &ContextMap, page_references: &Value) -> Value {
    let references = Value::from_iter([("page_references", page_references.clone())]);
    minijinja::value::merge_maps([as_template_value(context), references])
}

/// Final layout context, including the legacy `_context` snapshot.
pub fn as_layout_template_value(context: &ContextMap, page_references: &Value) -> Value {
    let page = as_template_value(context);
    let references = Value::from_iter([("page_references", page_references.clone())]);
    let legacy = minijinja::value::merge_maps([page.clone(), references.clone()]);
    minijinja::value::merge_maps([
        page,
        references,
        minijinja::value::Value::from_iter([("_context", legacy)]),
    ])
}

/// Builds the small deterministic context shared by HTML and Markdown pages.
/// Python-derived navigation, social data and image metadata are deliberately
/// not guessed here; they have explicit TODOs in the compatibility report.
pub fn build_base_context(
    config: &JsonValue,
    explicit: ContextMap,
    site_root: &Path,
    output_root: &Path,
    production: bool,
) -> ContextMap {
    let mut result = Map::new();
    if let Some(values) = config.as_object() {
        result.extend(values.clone());
        result.extend(config::runtime_aliases(config));
        result.insert("config".into(), config.clone());
    } else {
        result.insert("config".into(), config.clone());
    }
    result.insert(
        "site_root".into(),
        JsonValue::String(site_root.to_string_lossy().into_owned()),
    );
    result.insert(
        "output_root".into(),
        JsonValue::String(output_root.to_string_lossy().into_owned()),
    );
    result.insert(
        "environment".into(),
        JsonValue::String(
            if production {
                "production"
            } else {
                "development"
            }
            .into(),
        ),
    );
    result.insert("development_mode".into(), JsonValue::Bool(!production));
    // Python templates commonly probe optional special helpers before calling
    // them.  An empty object preserves that lenient lookup behavior; actual
    // macro calls are rewritten by the compatibility layer.
    result.insert("special".into(), JsonValue::Object(Map::new()));
    // Request context has the same practical role as the Python extra context
    // and therefore wins over config-derived defaults.
    for (key, value) in explicit {
        result.insert(key.to_ascii_lowercase(), value.clone());
        result.insert(key, value);
    }
    result
}

pub fn add_page_context(
    context: &mut ContextMap,
    source: &str,
    output: &std::path::Path,
    depth: i64,
    site_root: &Path,
) {
    let source_path = site_root.join(source);
    let output_string = output.to_string_lossy().replace('\\', "/");
    let route = url::route_for_output(&output_string);
    let clean_route = route.trim_matches('/');
    let mut route_parts = clean_route.rsplitn(2, '/');
    let page_name = route_parts.next().unwrap_or("index");
    let sub_dir = route_parts.next().unwrap_or("");
    context.insert("depth".into(), JsonValue::from(depth));
    context.insert("template_path".into(), JsonValue::String(source.into()));
    context.insert(
        "source_path".into(),
        JsonValue::String(source_path.to_string_lossy().into_owned()),
    );
    context.insert("output_path".into(), JsonValue::String(output_string));
    context.insert("page_name".into(), JsonValue::String(page_name.into()));
    context.insert("sub_dir".into(), JsonValue::String(sub_dir.into()));
    context.insert("current_path".into(), JsonValue::String(route.clone()));
    if let Some(base_url) = context.get("base_url").and_then(JsonValue::as_str) {
        let base = base_url.trim_end_matches('/');
        let current = format!("{}{route}", base);
        context.insert("current_url".into(), JsonValue::String(current.clone()));
        context.insert("canonical_url".into(), JsonValue::String(current));
    }
}

/// Expose the current page context for Python/Jinja's `attribute(_context, …)`
/// convention.  This is refreshed after page metadata has been added so
/// lookups see both config aliases and page-specific values.
pub fn add_legacy_context_object(context: &mut ContextMap) {
    let snapshot = JsonValue::Object(context.clone());
    context.insert("_context".into(), snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_context_wins_and_config_is_available() {
        let config = serde_json::json!({"base_url": "https://example.test", "title": "Config"});
        let mut explicit = Map::new();
        explicit.insert("title".into(), JsonValue::String("Request".into()));
        let result = build_base_context(
            &config,
            explicit,
            Path::new("/site"),
            Path::new("/out"),
            true,
        );
        assert_eq!(result["title"], "Request");
        assert_eq!(result["config"]["base_url"], "https://example.test");
        assert_eq!(result["development_mode"], false);
        assert!(result["special"].is_object());
    }

    #[test]
    fn legacy_context_object_contains_current_values() {
        let mut context = Map::new();
        context.insert("facebook_handle".into(), JsonValue::String("aw".into()));
        add_legacy_context_object(&mut context);
        assert_eq!(context["_context"]["facebook_handle"], "aw");
    }
}
