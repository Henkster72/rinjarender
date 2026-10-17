use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

const PUBLIC_ENV_KEYS: &[&str] = &[
    "BASE_URL",
    "TITLE_SITE",
    "SUBTITLE_SITE",
    "META_TITLE",
    "META_DESCRIPTION",
    "META_KEYWORDS",
    "FB_APP_ID",
    "MAIL_ADDRESS",
    "TEL_NO",
    "LOGO_IMAGE_URL",
    "PHYSICAL_ADDRESS",
    "WA_TEXT",
    "LINK_404",
    "TEXT_404",
    "TITLE_404",
    "COOKIE_ALERT",
    "COOKIE_TEXT",
    "GOOGLETAGID",
    "FACEBOOK_HANDLE",
    "INSTAGRAM_HANDLE",
    "YT_HANDLE",
    "PAYMENT_LINK",
];

const CANONICAL_ENV_KEYS: &[&str] = &[
    "BASE_URL",
    "TITLE_SITE",
    "SUBTITLE_SITE",
    "META_TITLE",
    "META_DESCRIPTION",
    "META_KEYWORDS",
];

pub fn load(site_root: &Path, config_path: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    let raw = if config_path.is_file() {
        serde_json::from_str(&fs::read_to_string(config_path)?)?
    } else {
        Value::Object(Map::new())
    };
    match raw.get("schema_version").and_then(Value::as_u64) {
        Some(2) => validate_v2(raw),
        Some(version) if version > 2 => Err(format!(
            "unsupported site_config schema_version {version}; supported through 2"
        )
        .into()),
        _ => normalize(site_root, raw),
    }
}

fn validate_v2(raw: Value) -> Result<Value, Box<dyn std::error::Error>> {
    let Some(config) = raw.as_object() else {
        return Err("site_config schema_version 2 must be a JSON object".into());
    };
    for key in [
        "base_url",
        "title_site",
        "subtitle_site",
        "language",
        "meta_title",
        "meta_description",
        "meta_keywords",
    ] {
        if config.contains_key(key) {
            return Err(format!(
                "schema_version 2 site_config must not contain flat compatibility alias {key}"
            )
            .into());
        }
    }
    for key in PUBLIC_ENV_KEYS {
        let flat = key.to_ascii_lowercase();
        if config.contains_key(&flat) {
            return Err(format!(
                "schema_version 2 site_config must keep public value {flat} under a structured section"
            )
            .into());
        }
    }
    if let Some(public) = config
        .get("site")
        .and_then(Value::as_object)
        .and_then(|site| site.get("public"))
        .and_then(Value::as_object)
    {
        for key in [
            "base_url",
            "name",
            "subtitle",
            "language",
            "title_site",
            "subtitle_site",
            "meta_title",
            "meta_description",
            "meta_keywords",
        ] {
            if public.contains_key(key) {
                return Err(format!(
                    "schema_version 2 site.public must not duplicate canonical value {key}"
                )
                .into());
            }
        }
    }
    Ok(raw)
}

pub fn normalize(site_root: &Path, raw: Value) -> Result<Value, Box<dyn std::error::Error>> {
    let mut config = raw.as_object().cloned().unwrap_or_default();
    config.entry("schema_version").or_insert(Value::from(2));
    let legacy = read_env(site_root);

    let site = object_mut(&mut config, "site");
    set_if_absent(site, "base_url", legacy_value(&legacy, "BASE_URL"));
    set_if_absent(site, "name", legacy_value(&legacy, "TITLE_SITE"));
    set_if_absent(site, "subtitle", legacy_value(&legacy, "SUBTITLE_SITE"));
    set_if_absent(site, "language", language_from_meta(site_root));

    let json_ld = if !config
        .get("seo")
        .and_then(Value::as_object)
        .is_some_and(|seo| seo.contains_key("json_ld"))
    {
        load_json_ld(site_root, &config)
    } else {
        None
    };
    let seo = object_mut(&mut config, "seo");
    set_if_absent(seo, "title", legacy_value(&legacy, "META_TITLE"));
    set_if_absent(
        seo,
        "description",
        legacy_value(&legacy, "META_DESCRIPTION"),
    );
    set_if_absent(seo, "keywords", legacy_value(&legacy, "META_KEYWORDS"));
    if let Some(value) = json_ld {
        seo.insert("json_ld".into(), value);
    }
    if !seo.contains_key("meta_tags") {
        if let Some(value) = load_meta_tags(site_root) {
            seo.insert("meta_tags".into(), value);
        }
    }

    let social = object_mut(&mut config, "social");
    if !social.contains_key("links") {
        if let Some(value) = load_social_csv(site_root.join("socialmedia.csv"))? {
            social.insert("links".into(), value);
        }
    }

    // Legacy configs retain the flat names until migration. Canonical v2
    // configs receive these names only through runtime_aliases().
    for key in PUBLIC_ENV_KEYS {
        let flat = key.to_ascii_lowercase();
        if !config.contains_key(&flat) {
            if let Some(value) = legacy.get(*key) {
                config.insert(flat, Value::String(value.clone()));
            }
        }
    }
    Ok(Value::Object(config))
}

pub fn migrate(site_root: &Path, config_path: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    let raw = if config_path.is_file() {
        serde_json::from_str(&fs::read_to_string(config_path)?)?
    } else {
        Value::Object(Map::new())
    };
    if raw.get("schema_version").and_then(Value::as_u64) == Some(2) {
        return validate_v2(raw);
    }
    let config = normalize(site_root, raw)?;
    let mut result = config.as_object().cloned().unwrap_or_default();
    result.remove("meta");
    for key in [
        "base_url",
        "title_site",
        "subtitle_site",
        "language",
        "meta_title",
        "meta_description",
        "meta_keywords",
    ] {
        result.remove(key);
    }
    for key in PUBLIC_ENV_KEYS {
        result.remove(&key.to_ascii_lowercase());
    }
    let mut public = Map::new();
    for key in PUBLIC_ENV_KEYS {
        if CANONICAL_ENV_KEYS.contains(key) {
            continue;
        }
        if let Some(value) = read_env(site_root).get(*key) {
            public.insert(key.to_ascii_lowercase(), Value::String(value.clone()));
        }
    }
    if !public.is_empty() {
        object_mut(&mut result, "site").insert("public".into(), Value::Object(public));
    }
    result.insert("schema_version".into(), Value::from(2));
    validate_v2(Value::Object(result))
}

/// Flat names retained for existing templates, but never persisted in a v2
/// site_config.json.
pub fn runtime_aliases(config: &Value) -> Map<String, Value> {
    let mut aliases = Map::new();
    let pairs = [
        ("base_url", "site.base_url"),
        ("title_site", "site.name"),
        ("subtitle_site", "site.subtitle"),
        ("language", "site.language"),
        ("meta_title", "seo.title"),
        ("meta_description", "seo.description"),
        ("meta_keywords", "seo.keywords"),
    ];
    let Some(root) = config.as_object() else {
        return aliases;
    };
    for (flat, nested) in pairs {
        if let Some(value) = dotted(root, nested) {
            aliases.insert(flat.into(), value.clone());
        }
    }
    if let Some(public) = root
        .get("site")
        .and_then(Value::as_object)
        .and_then(|site| site.get("public"))
        .and_then(Value::as_object)
    {
        aliases.extend(public.clone());
        for (group, fields) in [
            (
                "workshop_urls",
                ["gold", "diamond", "platinum", "private_lesson"],
            ),
            ("primary_cta", ["text", "url", "", ""]),
        ] {
            let Some(values) = public.get(group).and_then(Value::as_object) else {
                continue;
            };
            for field in fields.into_iter().filter(|field| !field.is_empty()) {
                if let Some(value) = values.get(field) {
                    let alias = if group == "workshop_urls" {
                        if field == "private_lesson" {
                            "private_lesson_url".to_owned()
                        } else {
                            format!("{field}_workshop_url")
                        }
                    } else {
                        format!("primary_cta_{field}")
                    };
                    aliases.entry(alias).or_insert_with(|| value.clone());
                }
            }
        }
    }
    if let Some(links) = root
        .get("social")
        .and_then(Value::as_object)
        .and_then(|social| social.get("links"))
        .and_then(Value::as_array)
    {
        for link in links {
            let Some(link) = link.as_object() else {
                continue;
            };
            let Some(name) = link.get("name").and_then(Value::as_str) else {
                continue;
            };
            let Some(handle) = link.get("handle").filter(|value| !value.is_null()) else {
                continue;
            };
            let handle = handle.as_str().unwrap_or_default();
            if handle.is_empty() {
                continue;
            }
            let alias = name
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() {
                        character.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
                .trim_matches('_')
                .to_owned();
            if !alias.is_empty() {
                aliases
                    .entry(format!("{alias}_handle"))
                    .or_insert_with(|| Value::String(handle.into()));
            }
        }
    }
    aliases
}

fn object_mut<'a>(root: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    let entry = root.entry(key).or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry.as_object_mut().expect("object inserted")
}

fn set_if_absent(target: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if !target.contains_key(key) {
        if let Some(value) = value {
            target.insert(key.into(), value);
        }
    }
}

fn dotted<'a>(root: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    let mut current = root.get(path.split('.').next()?)?;
    for part in path.split('.').skip(1) {
        current = current.get(part)?;
    }
    Some(current)
}

fn read_env(site_root: &Path) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    let path = site_root.join(".env");
    let Ok(text) = fs::read_to_string(path) else {
        return values;
    };
    for line in text.lines() {
        let Some((key, rest)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !PUBLIC_ENV_KEYS.contains(&key) {
            continue;
        }
        values.insert(
            key.into(),
            rest.split('|').next().unwrap_or("").trim().into(),
        );
    }
    values
}

fn legacy_value(values: &BTreeMap<String, String>, key: &str) -> Option<Value> {
    values.get(key).map(|value| Value::String(value.clone()))
}

fn language_from_meta(site_root: &Path) -> Option<Value> {
    let text = fs::read_to_string(site_root.join("meta.txt")).ok()?;
    text.lines().find_map(|line| {
        let mut parts = line.splitn(3, ';');
        (parts.next()? == "name" && parts.next()? == "language")
            .then(|| Value::String(parts.next().unwrap_or_default().trim().into()))
    })
}

fn load_json_ld(site_root: &Path, config: &Map<String, Value>) -> Option<Value> {
    let relative = config.get("meta")?.get("json_ld")?.as_str()?;
    let text = fs::read_to_string(site_root.join(relative)).ok()?;
    serde_json::from_str(&text).ok()
}

fn load_meta_tags(site_root: &Path) -> Option<Value> {
    let text = fs::read_to_string(site_root.join("meta.txt")).ok()?;
    let tags = text
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, ';');
            let kind = parts.next()?.trim();
            let name = parts.next()?.trim();
            let value = parts.next()?.trim();
            Some(serde_json::json!({ "kind": kind, "name": name, "value": value }))
        })
        .collect::<Vec<_>>();
    Some(Value::Array(tags))
}

fn load_social_csv(path: PathBuf) -> Result<Option<Value>, Box<dyn std::error::Error>> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)?;
    let mut lines = text.lines();
    let headers: Vec<&str> = lines.next().unwrap_or_default().split(',').collect();
    let rows = lines
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').collect();
            if fields.len() != headers.len() {
                return None;
            }
            let mut row = Map::new();
            for (key, value) in headers.iter().zip(fields) {
                row.insert((*key).into(), Value::String(value.trim().into()));
            }
            Some(Value::Object(row))
        })
        .collect::<Vec<_>>();
    Ok(Some(Value::Array(rows)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_public_values_are_aliases_but_secrets_are_not_loaded() {
        let root = std::env::temp_dir().join(format!("jnja-config-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(".env"),
            "BASE_URL=https://example.test|crucial\nSSH_PWD=test-value|crucial\n",
        )
        .unwrap();
        fs::write(root.join("meta.txt"), "name;language;nl\n").unwrap();
        let value = normalize(&root, Value::Object(Map::new())).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert_eq!(value["base_url"], "https://example.test");
        assert_eq!(value["site"]["language"], "nl");
        assert!(value.get("ssh_pwd").is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v2_does_not_read_legacy_files_or_persist_aliases() {
        let root = std::env::temp_dir().join(format!("jnja-config-v2-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(".env"), "BASE_URL=https://legacy.invalid\n").unwrap();
        fs::write(root.join("meta.txt"), "name;language;xx\n").unwrap();
        fs::write(
            root.join("socialmedia.csv"),
            "name,url\nlegacy,https://invalid\n",
        )
        .unwrap();
        let path = root.join("site_config.json");
        fs::write(
            &path,
            r#"{"schema_version":2,"site":{"base_url":"https://canonical.test","language":"nl"},"seo":{"title":"Canonical"}}"#,
        )
        .unwrap();
        let value = load(&root, &path).unwrap();
        assert_eq!(value["site"]["base_url"], "https://canonical.test");
        assert!(value.get("base_url").is_none());
        assert!(value.get("social").is_none());
        assert_eq!(
            runtime_aliases(&value)["base_url"],
            "https://canonical.test"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v2_social_links_provide_runtime_handle_aliases() {
        let value = serde_json::json!({
            "schema_version": 2,
            "social": {"links": [{"name": "sendfox", "handle": "Example"}]}
        });
        assert_eq!(runtime_aliases(&value)["sendfox_handle"], "Example");
    }

    #[test]
    fn v2_structured_public_groups_provide_legacy_aliases() {
        let value = serde_json::json!({
            "schema_version": 2,
            "site": {"base_url": "https://canonical.example", "name": "Canonical site", "public": {
                "maps_shortcut": "https://maps.example",
                "header_title": "Header",
                "header_description": "Description",
                "header_keywords": "keywords",
                "contact_email": "contact@example",
                "workshop_urls": {"private_lesson": "https://example.test/book"},
                "primary_cta": {"text": "Book", "url": "#book"}
            }}
        });
        let aliases = runtime_aliases(&value);
        assert_eq!(aliases["base_url"], "https://canonical.example");
        assert_eq!(aliases["title_site"], "Canonical site");
        assert_eq!(aliases["maps_shortcut"], "https://maps.example");
        assert_eq!(aliases["header_title"], "Header");
        assert_eq!(aliases["header_description"], "Description");
        assert_eq!(aliases["header_keywords"], "keywords");
        assert_eq!(aliases["contact_email"], "contact@example");
        assert_eq!(aliases["private_lesson_url"], "https://example.test/book");
        assert_eq!(aliases["primary_cta_text"], "Book");
        assert_eq!(aliases["primary_cta_url"], "#book");
    }

    #[test]
    fn migration_keeps_canonical_values_out_of_public_duplicates() {
        let root = std::env::temp_dir().join(format!("jnja-config-migrate-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(".env"),
            "BASE_URL=https://canonical.test\nTITLE_SITE=Canonical\nMETA_TITLE=Canonical title\nMAIL_ADDRESS=mail@example.test\n",
        )
        .unwrap();
        fs::write(root.join("meta.txt"), "name;language;nl\n").unwrap();
        let value = migrate(&root, &root.join("site_config.json")).unwrap();
        assert_eq!(value["site"]["base_url"], "https://canonical.test");
        assert_eq!(value["site"]["name"], "Canonical");
        assert_eq!(value["seo"]["title"], "Canonical title");
        assert_eq!(value["site"]["language"], "nl");
        assert_eq!(value["site"]["public"]["mail_address"], "mail@example.test");
        for key in [
            "base_url",
            "title_site",
            "subtitle_site",
            "language",
            "meta_title",
            "meta_description",
            "meta_keywords",
        ] {
            assert!(value.get(key).is_none(), "legacy alias persisted: {key}");
            assert!(value["site"]["public"].get(key).is_none());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_newer_schema_versions() {
        let root = std::env::temp_dir().join(format!("jnja-config-newer-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("site_config.json");
        fs::write(&path, r#"{"schema_version":3}"#).unwrap();
        assert!(load(&root, &path).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn v2_accepts_editor_schema_metadata() {
        let root = std::env::temp_dir().join(format!("jnja-config-schema-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("site_config.json");
        fs::write(
            &path,
            r#"{"$schema":"https://example.test/site-config.schema.json","schema_version":2}"#,
        )
        .unwrap();
        let value = load(&root, &path).unwrap();
        assert_eq!(value["schema_version"], 2);
        assert_eq!(
            value["$schema"],
            "https://example.test/site-config.schema.json"
        );
        let schema: Value =
            serde_json::from_str(include_str!("../schemas/site-config-v2.schema.json")).unwrap();
        assert_eq!(schema["properties"]["schema_version"]["const"], 2);
        fs::remove_dir_all(root).unwrap();
    }
}
