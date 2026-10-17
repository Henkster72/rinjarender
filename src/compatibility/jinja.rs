use crate::url;
use minijinja::value::Value;
use minijinja::{Environment, UndefinedBehavior};
use std::{fs, path::Path};
pub fn configure(env: &mut Environment<'static>) {
    env.set_undefined_behavior(UndefinedBehavior::Lenient);
    env.add_filter("static", |path: String, depth: i64| {
        static_path(&path, depth)
    });
    env.add_filter("url", |path: String, depth: i64| {
        url::page_route(&path, depth)
    });
    env.add_filter("startswith", |value: String, prefix: String| {
        value.starts_with(&prefix)
    });
    env.add_filter("endswith", |value: String, suffix: String| {
        value.ends_with(&suffix)
    });
    env.add_filter("split", |value: String, separator: String| {
        value
            .split(&separator)
            .map(str::to_owned)
            .collect::<Vec<_>>()
    });
    env.add_filter(
        "replace",
        |value: String, old: String, new: String, count: Option<i64>| match count {
            Some(limit) if limit >= 0 => value.replacen(&old, &new, limit as usize),
            _ => value.replace(&old, &new),
        },
    );
    env.add_filter("striptags", strip_tags);
    env.add_function("attribute", |value: Value, name: String| {
        value.get_attr(&name)
    });
    // ui, special and exact Markup/safe rules remain TODOs until corpus tests.
}

fn static_path(path: &str, depth: i64) -> String {
    let path = path.trim().replace('\\', "/");
    if path.is_empty() {
        return format!("{}static/", "../".repeat(depth.max(0) as usize));
    }
    if path.starts_with("http://") || path.starts_with("https://") || path.starts_with("//") {
        return path;
    }
    let normalized = path.trim_start_matches('/');
    let lower = normalized.to_ascii_lowercase();
    let image = [
        ".jpg", ".jpeg", ".webp", ".avif", ".png", ".gif", ".mp4", ".mov", ".ico", ".svg",
    ]
    .iter()
    .any(|suffix| lower.ends_with(suffix));
    let relative = if normalized.starts_with("static/") {
        normalized.to_owned()
    } else if normalized.contains('/') || !image {
        format!("static/{normalized}")
    } else {
        format!("static/images/{normalized}")
    };
    format!("{}{}", "../".repeat(depth.max(0) as usize), relative)
}

fn strip_tags(value: String) -> String {
    let mut result = String::with_capacity(value.len());
    let mut inside_tag = false;
    for character in value.chars() {
        match character {
            '<' => inside_tag = true,
            '>' if inside_tag => inside_tag = false,
            _ if !inside_tag => result.push(character),
            _ => {}
        }
    }
    result
}

pub fn prepare_template(source: &str, template_root: &Path) -> String {
    let mut prepared = source.to_owned();
    if let Ok(entries) = fs::read_dir(template_root.join("macros")) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if !name.starts_with("macro_") || !name.ends_with(".html") {
                continue;
            }
            let Ok(macro_source) = fs::read_to_string(&path) else {
                continue;
            };
            let Some(macro_name) = name
                .strip_prefix("macro_")
                .and_then(|value| value.strip_suffix(".html"))
            else {
                continue;
            };
            let call = format!("special.{macro_name}(");
            if !contains_active_call(&prepared, &call) || !macro_source.contains("{% macro render")
            {
                continue;
            }
            let alias = format!("_special_{macro_name}");
            prepared = format!(
                "{{% import \"macros/{name}\" as {alias} %}}\n{}",
                prepared.replace(&call, &format!("{alias}.render("))
            );
        }
    }
    let prepared = rewrite_map_gets(&prepared);
    let prepared = prepared
        .replace(
            "sub.update({'slug': sub.href.split('?')[0].split('.')[0]|lower })",
            "sub.href",
        )
        .replace(
            "sub.update({'slug': sub.href.split('?')[0].split('.')[0] | lower })",
            "sub.href",
        )
        .replace(
            "sub_slugs.append(sub.slug|lower)",
            "sub_slugs.append(sub.href | replace('.html', '') | lower)",
        )
        .replace(
            "active_page_lower == sub.slug|lower",
            "active_page_lower == (sub.href | replace('.html', '') | lower)",
        )
        .replace(
            "sub_slugs.append(sub.href | replace('.html', '') | lower)",
            "sub_slugs = sub_slugs + [sub.href | replace('.html', '') | lower]",
        )
        .replace(
            "{% set _ = sub_slugs = sub_slugs + [sub.href | replace('.html', '') | lower] %}",
            "{% set sub_slugs = sub_slugs + [sub.href | replace('.html', '') | lower] %}",
        );
    rewrite_jinja_string_methods(&prepared)
}

fn contains_active_call(source: &str, call: &str) -> bool {
    let mut rest = source;
    while let Some(start) = rest.find("{#") {
        if rest[..start].contains(call) {
            return true;
        }
        let Some(end) = rest[start + 2..].find("#}") else {
            return false;
        };
        rest = &rest[start + 2 + end + 2..];
    }
    rest.contains(call)
}

fn rewrite_jinja_string_methods(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    while cursor < source.len() {
        let remainder = &source[cursor..];
        let next_variable = remainder.find("{{");
        let next_block = remainder.find("{%");
        let Some((relative, marker)) = (match (next_variable, next_block) {
            (Some(variable), Some(block)) if variable <= block => Some((variable, "{{")),
            (Some(_), Some(block)) => Some((block, "{%")),
            (Some(variable), None) => Some((variable, "{{")),
            (None, Some(block)) => Some((block, "{%")),
            (None, None) => None,
        }) else {
            output.push_str(&source[cursor..]);
            break;
        };
        let start = cursor + relative;
        output.push_str(&source[cursor..start]);
        let close = if marker == "{{" { "}}" } else { "%}" };
        let Some(end_relative) = source[start + 2..].find(close) else {
            output.push_str(&source[start..]);
            break;
        };
        let end = start + 2 + end_relative;
        let expression = &source[start..end];
        let expression = expression
            .replace(".startswith(", " | startswith(")
            .replace(".endswith(", " | endswith(")
            .replace(".split(", " | split(");
        output.push_str(&expression);
        output.push_str(close);
        cursor = end + 2;
    }
    output
}

fn rewrite_map_gets(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    while let Some(relative) = source[cursor..].find(".get(") {
        let start = cursor + relative;
        output.push_str(&source[cursor..start]);
        let rest = &source[start + 5..];
        let Some(quote) = rest
            .chars()
            .next()
            .filter(|value| *value == '\'' || *value == '"')
        else {
            output.push_str(".get(");
            cursor = start + 5;
            continue;
        };
        let Some(end_quote) = rest[1..].find(quote) else {
            output.push_str(".get(");
            cursor = start + 5;
            continue;
        };
        let key = &rest[1..end_quote + 1];
        let mut nesting = 1usize;
        let mut close = None;
        let mut quote_state = None;
        for (index, character) in rest[end_quote + 2..].char_indices() {
            if let Some(active_quote) = quote_state {
                if character == active_quote {
                    quote_state = None;
                }
                continue;
            }
            if character == '\'' || character == '"' {
                quote_state = Some(character);
            } else if character == '(' {
                nesting += 1;
            } else if character == ')' {
                nesting -= 1;
                if nesting == 0 {
                    close = Some(index);
                    break;
                }
            }
        }
        let Some(close) = close else {
            output.push_str(".get(");
            cursor = start + 5;
            continue;
        };
        let close_absolute = end_quote + 2 + close;
        let default = rest[end_quote + 2..close_absolute]
            .trim()
            .strip_prefix(',')
            .map(str::trim)
            .filter(|value| !value.is_empty());
        output.push_str(&format!("[\"{}\"]", key.replace('"', "\\\"")));
        if let Some(default) = default {
            output.push_str(" | default(");
            output.push_str(&rewrite_map_gets(default));
            output.push(')');
        }
        cursor = start + 5 + end_quote + 2 + close + 1;
    }
    output.push_str(&source[cursor..]);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_match_python_depth_conventions() {
        assert_eq!(static_path("image.webp", 1), "../static/images/image.webp");
        assert_eq!(url::page_route("page.html", 1), "../page/");
    }

    #[test]
    fn rewrites_nested_mapping_get_calls() {
        let prepared =
            rewrite_map_gets("{{ site_header.get('brandImageAlt', site_header.get('brand')) }}");
        assert_eq!(
            prepared,
            "{{ site_header[\"brandImageAlt\"] | default(site_header[\"brand\"]) }}"
        );
    }

    #[test]
    fn mapping_get_uses_string_default_when_key_is_missing() {
        let mut env = Environment::new();
        configure(&mut env);
        let source = prepare_template(
            "{{ data.get('backgroundAnimationSvg', 'images/world_nl.svg') }}",
            Path::new("/does-not-exist"),
        );
        let rendered = env
            .render_str(&source, minijinja::context! { data => {} })
            .unwrap();
        assert_eq!(rendered, "images/world_nl.svg");
    }

    #[test]
    fn mapping_get_preserves_present_values_and_explicit_null() {
        let mut env = Environment::new();
        configure(&mut env);
        let source = prepare_template(
            "{{ data.get('name', 'fallback') }}|{{ data.get('missing', 8) }}|{{ data.get('null_value', 'fallback') }}",
            Path::new("/does-not-exist"),
        );
        let data = serde_json::json!({"name": "stored", "null_value": null});
        let rendered = env
            .render_str(&source, minijinja::context! { data => data })
            .unwrap();
        assert_eq!(rendered, "stored|8|None");
    }

    #[test]
    fn mapping_get_numeric_default_can_flow_through_asset_src() {
        let mut env = Environment::new();
        configure(&mut env);
        let source = prepare_template(
            "{{ data.get('backgroundAnimationZoomPadding', 8) }}|{{ data.get('backgroundAnimationSvg', 'images/world_nl.svg') | static(depth) }}",
            Path::new("/does-not-exist"),
        );
        let rendered = env
            .render_str(&source, minijinja::context! { data => {}, depth => 1 })
            .unwrap();
        assert_eq!(rendered, "8|../static/images/world_nl.svg");
    }

    #[test]
    fn ignores_special_examples_inside_comments() {
        assert!(!contains_active_call(
            "{# usage: {{ special.phone('+31') }} #}",
            "special.phone("
        ));
        assert!(contains_active_call(
            "{{ special.phone('+31') }}",
            "special.phone("
        ));
    }

    #[test]
    fn replace_supports_python_count_argument() {
        let mut env = Environment::new();
        configure(&mut env);
        let rendered = env
            .render_str(
                "{{ value | replace('a', 'x', 1) }}",
                minijinja::context! { value => "aaa" },
            )
            .unwrap();
        assert_eq!(rendered, "xaa");
    }

    #[test]
    fn page_route_filter_and_namespace_preserve_hyphenated_route() {
        let mut env = Environment::new();
        configure(&mut env);
        let source = crate::compatibility::markdown::rewrite_asset_tokens_runtime(
            "{% set meet_brooklin = namespace(html='meet-brooklin.html' | url(depth)) %}{{ meet_brooklin.html }}",
        );
        let prepared = prepare_template(&source, Path::new("/does-not-exist"));
        let rendered = env
            .render_str(&prepared, minijinja::context! { depth => 0 })
            .unwrap();
        assert_eq!(rendered, "meet-brooklin/");
        assert_eq!(url::page_route("meet-brooklin.html", 0), "meet-brooklin/");
    }

    #[test]
    fn striptags_removes_markup_without_changing_text() {
        let mut env = Environment::new();
        configure(&mut env);
        let rendered = env
            .render_str(
                "{{ value | striptags }}",
                minijinja::context! {
                    value => "<p>Hello <strong>world</strong></p>"
                },
            )
            .unwrap();
        assert_eq!(rendered, "Hello world");
    }

    #[test]
    fn optional_special_namespace_and_context_attribute_match_python_lookup() {
        let mut env = Environment::new();
        configure(&mut env);
        let rendered = env
            .render_str(
                "{{ attribute(_context, 'facebook_handle') }}{% if special.header %}bad{% endif %}",
                minijinja::context! {
                    _context => minijinja::context! { facebook_handle => "aw" },
                    special => minijinja::context! {},
                },
            )
            .unwrap();
        assert_eq!(rendered, "aw");
    }
}
