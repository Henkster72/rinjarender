use comrak::Options;
use std::collections::HashSet;

const ATTR_START: &str = "__JNJA_ATTR_START__";
const HEADING_ID_START: &str = "__JNJA_HEADING_ID_START__";
const INLINE_ATTR_START: &str = "JNJAATTRSTART";
const INLINE_ATTR_END: &str = "JNJAATTREND";
pub fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.footnotes = true;
    options.extension.header_id_prefix = Some(String::new());
    options.extension.header_attributes = true;
    options.render.r#unsafe = true;
    options
}
fn is_asset_extension(extension: &str) -> bool {
    matches!(
        extension,
        "jpg"
            | "jpeg"
            | "webp"
            | "avif"
            | "png"
            | "gif"
            | "mp4"
            | "mov"
            | "ico"
            | "svg"
            | "css"
            | "js"
            | "woff2"
            | "html"
            | "pdf"
    )
}

fn asset_token_end(source: &str, start: usize) -> Option<usize> {
    let end = source[start + 2..].find("}}")? + start + 2;
    let path = source[start + 2..end].trim();
    let (stem, extension) = path.rsplit_once('.')?;
    if stem.is_empty() || !is_asset_extension(&extension.to_ascii_lowercase()) {
        return None;
    }
    // An expression such as ``{{ meet_brooklin.html }}`` is a Jinja
    // namespace attribute, not a literal asset token. Rewriting it before
    // MiniJinja evaluates the expression turns the variable name into a URL.
    if matches!(extension.to_ascii_lowercase().as_str(), "html" | "htm")
        && !stem.contains('/')
        && stem.contains('_')
        && stem
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
    {
        return None;
    }
    if path.chars().all(|character| {
        character.is_alphanumeric() || matches!(character, '/' | '-' | '_' | '.' | '(' | ')' | '%')
    }) {
        Some(end + 2)
    } else {
        None
    }
}

pub fn rewrite_asset_tokens(source: &str, depth: i64) -> String {
    rewrite_asset_tokens_with_depth(source, &depth.to_string())
}

pub fn rewrite_asset_tokens_runtime(source: &str) -> String {
    rewrite_asset_tokens_with_depth(source, "depth")
}

fn rewrite_asset_tokens_with_depth(source: &str, depth: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    while let Some(relative) = source[cursor..].find("{{") {
        let start = cursor + relative;
        output.push_str(&source[cursor..start]);
        let Some(end) = asset_token_end(source, start) else {
            output.push_str("{{");
            cursor = start + 2;
            continue;
        };
        let token = source[start + 2..end - 2].trim();
        let filter = if token.ends_with(".html") {
            "url"
        } else {
            "static"
        };
        output.push_str(&format!("{{{{ '{token}' | {filter}({depth}) }}}}"));
        cursor = end;
    }
    output.push_str(&source[cursor..]);
    output
}

pub fn preprocess(source: &str, depth: i64) -> String {
    let _ = depth;
    rewrite_attribute_lists(&normalize_heading_attributes(
        &rewrite_asset_tokens_runtime(source),
    ))
    .replace("{:", ATTR_START)
    .replace("{#", HEADING_ID_START)
}

pub fn restore_protected(source: &str) -> String {
    source
        .replace(ATTR_START, "{:")
        .replace(HEADING_ID_START, "{#")
}
pub fn postprocess(html: String, markdown_source: &str) -> String {
    rewrite_heading_anchors(apply_marked_attributes(html), markdown_source)
}

fn rewrite_attribute_lists(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut fenced = false;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        if fence {
            fenced = !fenced;
            output.push_str(line);
            continue;
        }
        let is_heading = trimmed.starts_with('#');
        if fenced {
            output.push_str(line);
            continue;
        }
        output.push_str(&rewrite_attribute_line(line, is_heading));
    }
    if !source.ends_with('\n') && output.ends_with('\n') {
        output.pop();
    }
    output
}

fn rewrite_attribute_line(line: &str, is_heading: bool) -> String {
    let mut output = String::with_capacity(line.len());
    let mut cursor = 0;
    let mut code_ticks = 0;
    while cursor < line.len() {
        let rest = &line[cursor..];
        let Some(relative) = rest.find(['`', '{']) else {
            output.push_str(rest);
            break;
        };
        let position = cursor + relative;
        output.push_str(&line[cursor..position]);
        let character = line.as_bytes()[position] as char;
        if character == '`' {
            let ticks = line[position..]
                .bytes()
                .take_while(|byte| *byte == b'`')
                .count();
            if code_ticks == 0 {
                code_ticks = ticks;
            } else if ticks == code_ticks {
                code_ticks = 0;
            }
            output.push_str(&line[position..position + ticks]);
            cursor = position + ticks;
            continue;
        }
        if code_ticks != 0 || line.as_bytes().get(position + 1) == Some(&b'{') {
            output.push('{');
            cursor = position + 1;
            continue;
        }
        let Some(end_relative) = line[position + 1..].find('}') else {
            output.push('{');
            cursor = position + 1;
            continue;
        };
        let end = position + 1 + end_relative;
        let inner = &line[position + 1..end];
        let Some(attributes) = parse_attribute_list(inner) else {
            output.push('{');
            cursor = position + 1;
            continue;
        };
        if is_heading && !output.trim_end().ends_with(')') {
            output.push_str(&line[position..=end]);
            cursor = end + 1;
            continue;
        }
        output.push_str(&encode_attribute_marker(&attributes));
        cursor = end + 1;
    }
    output
}

fn parse_attribute_list(raw: &str) -> Option<Vec<(String, String)>> {
    let mut input = raw.trim();
    if let Some(rest) = input.strip_prefix(':') {
        input = rest.trim_start();
    }
    if input.is_empty() {
        return None;
    }
    // WPM exports occasionally escape the quotes in an attr_list value. Treat
    // those as the same quoted value Python-Markdown receives.
    let input = input.replace("\\\"", "\"").replace("\\'", "'");
    let chars = input.chars().collect::<Vec<_>>();
    let mut index = 0;
    let mut attributes = Vec::new();
    let mut classes = Vec::new();
    while index < chars.len() {
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        if index == chars.len() {
            break;
        }
        if matches!(chars[index], '#' | '.') {
            let kind = chars[index];
            index += 1;
            let start = index;
            while index < chars.len() && is_attribute_name_char(chars[index]) {
                index += 1;
            }
            if start == index {
                return None;
            }
            let value = chars[start..index].iter().collect::<String>();
            if kind == '#' {
                attributes.push(("id".into(), value));
            } else {
                classes.push(value);
            }
            continue;
        }
        let start = index;
        while index < chars.len() && is_attribute_name_char(chars[index]) {
            index += 1;
        }
        if start == index || chars.get(index) != Some(&'=') {
            return None;
        }
        let name = chars[start..index].iter().collect::<String>();
        index += 1;
        let value = if matches!(chars.get(index), Some('\'' | '"')) {
            let quote = chars[index];
            index += 1;
            let start = index;
            while index < chars.len() && chars[index] != quote {
                index += 1;
            }
            if index == chars.len() {
                return None;
            }
            let value = chars[start..index].iter().collect::<String>();
            index += 1;
            value
        } else {
            let start = index;
            while index < chars.len() && !chars[index].is_whitespace() {
                index += 1;
            }
            if start == index {
                return None;
            }
            chars[start..index].iter().collect::<String>()
        };
        attributes.push((name, value));
    }
    if !classes.is_empty() {
        attributes.push(("class".into(), classes.join(" ")));
    }
    (!attributes.is_empty()).then_some(attributes)
}

fn is_attribute_name_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | ':')
}

fn encode_attribute_marker(attributes: &[(String, String)]) -> String {
    let encoded = attributes
        .iter()
        .map(|(name, value)| format!("{}={}", hex_encode(name), hex_encode(value)))
        .collect::<Vec<_>>()
        .join(";");
    format!("{INLINE_ATTR_START}{encoded}{INLINE_ATTR_END}")
}

fn hex_encode(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_attribute_marker(value: &str) -> Option<Vec<(String, String)>> {
    value
        .split(';')
        .map(|pair| {
            let (name, value) = pair.split_once('=')?;
            Some((hex_decode(name)?, hex_decode(value)?))
        })
        .collect()
}

fn hex_decode(value: &str) -> Option<String> {
    if value.len() % 2 != 0 {
        return None;
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

fn apply_marked_attributes(mut html: String) -> String {
    while let Some(start) = html.find(INLINE_ATTR_START) {
        let content_start = start + INLINE_ATTR_START.len();
        let Some(end_relative) = html[content_start..].find(INLINE_ATTR_END) else {
            break;
        };
        let end = content_start + end_relative + INLINE_ATTR_END.len();
        let Some(attributes) =
            decode_attribute_marker(&html[content_start..content_start + end_relative])
        else {
            break;
        };
        let before = &html[..start];
        let trailing_ws_start = before.trim_end_matches(char::is_whitespace).len();
        let target_end = trailing_ws_start;
        let target = &html[..target_end];
        let target_tag = if target.ends_with("</a>") {
            Some("a")
        } else if target.ends_with('>') {
            Some("img")
        } else {
            None
        };
        if let Some(tag) = target_tag {
            let opening = find_last_opening_tag(&html, target_end, tag);
            if opening.is_none() {
                break;
            }
            html.replace_range(trailing_ws_start..end, "");
            let (open_start, open_end) = opening.expect("opening tag checked above");
            apply_attributes(&mut html, open_start, open_end, &attributes);
        } else if let Some((open_start, open_end, close_end)) = current_block(&html, start) {
            let marker_start = start;
            let marker_end = end;
            html.replace_range(marker_start..marker_end, "");
            apply_attributes(&mut html, open_start, open_end, &attributes);
            let _ = close_end;
        } else {
            break;
        }
    }
    html
}

fn find_last_opening_tag(html: &str, before_end: usize, tag: &str) -> Option<(usize, usize)> {
    let needle = format!("<{tag}");
    let open_start = html[..before_end].rfind(&needle)?;
    let open_end_relative = html[open_start..before_end].find('>')?;
    let open_end = open_start + open_end_relative;
    Some((open_start, open_end))
}

fn current_block(html: &str, marker_start: usize) -> Option<(usize, usize, usize)> {
    let open_start = html[..marker_start].rfind("<p")?;
    let open_end = open_start + html[open_start..marker_start].find('>')?;
    let close_start = marker_start + html[marker_start..].find("</p>")?;
    Some((open_start, open_end, close_start + 4))
}

fn apply_attributes(
    html: &mut String,
    open_start: usize,
    open_end: usize,
    attributes: &[(String, String)],
) {
    let suffix = attributes
        .iter()
        .map(|(name, value)| format!(" {name}=\"{}\"", html_escape(value)))
        .collect::<String>();
    let mut insertion = open_end;
    while insertion > open_start && html.as_bytes()[insertion - 1].is_ascii_whitespace() {
        insertion -= 1;
    }
    if insertion > open_start && html.as_bytes()[insertion - 1] == b'/' {
        insertion -= 1;
    }
    html.insert_str(insertion, &suffix);
}

fn normalize_heading_attributes(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('#') {
                return line.to_owned();
            }
            let Some(start) = line.rfind("{:") else {
                return line.to_owned();
            };
            let Some(end) = line[start..].find('}') else {
                return line.to_owned();
            };
            let attrs = &line[start + 2..start + end];
            let id = attrs
                .split_whitespace()
                .find_map(|value| value.strip_prefix('#'));
            let remaining = attrs
                .split_whitespace()
                .filter(|value| value.strip_prefix('#') != id)
                .collect::<Vec<_>>();
            let replacement = if let Some(id) = id {
                if remaining.is_empty() {
                    format!("{{#{id}}}")
                } else {
                    format!("{{#{id} {}}}", remaining.join(" "))
                }
            } else {
                format!("{{{}}}", attrs.trim())
            };
            format!(
                "{} {}{}",
                line[..start].trim_end(),
                replacement,
                &line[start + end + 1..]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn explicit_heading_ids(source: &str) -> Vec<Option<String>> {
    source
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('#') {
                return None;
            }
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            if !(1..=6).contains(&level) || trimmed.chars().nth(level) != Some(' ') {
                return None;
            }
            heading_attribute_list(line).and_then(|attrs| {
                attrs
                    .into_iter()
                    .find(|(name, _)| name == "id")
                    .map(|(_, value)| value)
            })
        })
        .collect()
}

fn explicit_heading_attributes(source: &str) -> Vec<Option<Vec<(String, String)>>> {
    source
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('#') {
                return None;
            }
            Some(heading_attribute_list(line))
        })
        .collect()
}

fn heading_attribute_list(line: &str) -> Option<Vec<(String, String)>> {
    let start = line.rfind('{')?;
    let end = line[start..].find('}')?;
    parse_attribute_list(&line[start + 1..start + end])
}

fn slugify(text: &str, used: &mut HashSet<String>) -> String {
    let normalized = text
        .chars()
        .flat_map(|character| character.to_lowercase())
        .flat_map(transliterate)
        .map(|character| {
            if character.is_ascii_alphanumeric()
                || character == '_'
                || character == '-'
                || character.is_whitespace()
            {
                character
            } else {
                ' '
            }
        })
        .collect::<String>();
    let mut slug = normalized.split_whitespace().collect::<Vec<_>>().join("-");
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    while used.contains(&slug) || slug.is_empty() {
        let mut suffix = 1;
        if let Some((base, number)) = slug.rsplit_once('_') {
            if let Ok(value) = number.parse::<u32>() {
                suffix = value + 1;
                slug = base.to_owned();
            }
        }
        slug = format!("{slug}_{suffix}");
    }
    used.insert(slug.clone());
    slug
}

fn transliterate(character: char) -> std::vec::IntoIter<char> {
    let replacement = match character {
        'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' | 'À' | 'Á' | 'Â' | 'Ä' | 'Ã' | 'Å' => "a",
        'ç' | 'Ç' => "c",
        'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => "e",
        'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => "i",
        'ñ' | 'Ñ' => "n",
        'ò' | 'ó' | 'ô' | 'ö' | 'õ' | 'Ò' | 'Ó' | 'Ô' | 'Ö' | 'Õ' => "o",
        'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => "u",
        'ý' | 'ÿ' | 'Ý' | 'Ÿ' => "y",
        _ => return std::iter::once(character).collect::<Vec<_>>().into_iter(),
    };
    replacement.chars().collect::<Vec<_>>().into_iter()
}

fn plain_text(html: &str) -> String {
    let mut output = String::new();
    let mut chars = html.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '<' {
            for next in chars.by_ref() {
                if next == '>' {
                    break;
                }
            }
        } else {
            output.push(character);
        }
    }
    output
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn rewrite_heading_anchors(mut html: String, markdown_source: &str) -> String {
    let explicit = explicit_heading_ids(markdown_source);
    let explicit_attributes = explicit_heading_attributes(markdown_source);
    let mut explicit_index = 0;
    let mut used = HashSet::new();
    let mut cursor = 0;
    while let Some(relative) = html[cursor..].find("<h") {
        let start = cursor + relative;
        let Some(open_end_relative) = html[start..].find('>') else {
            break;
        };
        let open_end = start + open_end_relative;
        let Some(level) = html[start + 2..].chars().next() else {
            break;
        };
        let Some(close_relative) = html[open_end + 1..].find(&format!("</h{level}>")) else {
            break;
        };
        let close_start = open_end + 1 + close_relative;
        let close_end = close_start + 5;
        let inner = html[open_end + 1..close_start].to_owned();
        let current_id = html[start..open_end]
            .split_once(" id=\"")
            .and_then(|(_, rest)| rest.split_once('"').map(|(id, _)| id.to_owned()));
        if let Some(current_id) = current_id {
            let explicit_id = explicit.get(explicit_index).cloned().flatten();
            explicit_index += 1;
            let text = plain_text(&inner);
            let target_id = explicit_id.unwrap_or_else(|| slugify(&text, &mut used));
            used.insert(target_id.clone());
            let mut open = html[start..=open_end].to_owned();
            open = open.replace(
                &format!("id=\"{current_id}\""),
                &format!("id=\"{target_id}\""),
            );
            if let Some(attributes) = explicit_attributes
                .get(explicit_index - 1)
                .and_then(Option::as_ref)
            {
                let attributes = attributes
                    .iter()
                    .filter(|(name, _)| name != "id")
                    .cloned()
                    .collect::<Vec<_>>();
                if !attributes.is_empty() {
                    let open_end = open.len() - 1;
                    apply_attributes(&mut open, 0, open_end, &attributes);
                }
            }
            let anchor_start = inner.rfind(&format!("<a href=\"#{current_id}\""));
            let content = if let Some(anchor_start) = anchor_start {
                if inner[anchor_start..].contains("class=\"anchor\"") {
                    inner[..anchor_start].to_owned()
                } else {
                    inner.clone()
                }
            } else {
                inner.clone()
            };
            let replacement = format!(
                "{open}<a class=\"toclink\" href=\"#{target_id}\">{content}</a><a class=\"headerlink\" href=\"#{target_id}\" title=\"Permanent link\">&para;</a></h{level}>"
            );
            html.replace_range(start..close_end, &replacement);
            cursor = start + replacement.len();
        } else {
            cursor = close_end;
        }
    }
    html
}

pub fn toc_from_html(html: &str) -> Option<String> {
    let mut entries = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = html[cursor..].find("<h") {
        let start = cursor + relative;
        let level = html[start + 2..].chars().next()?;
        let open_end = start + html[start..].find('>')?;
        let close_start = open_end + 1 + html[open_end + 1..].find(&format!("</h{level}>"))?;
        let id = html[start..open_end]
            .split_once(" id=\"")?
            .1
            .split_once('"')?
            .0;
        let inner = html[open_end + 1..close_start]
            .split_once("</a><a class=\"headerlink\"")
            .map(|(v, _)| v)
            .unwrap_or("");
        let title = plain_text(inner).trim().to_owned();
        entries.push((level.to_digit(10)? as usize, id.to_owned(), title));
        cursor = close_start + 5;
    }
    if entries.is_empty() {
        return None;
    }
    let mut result = String::from("<div class=\"toc\">\n<ul>\n");
    let mut previous = entries[0].0;
    let mut open = 1;
    for (index, (level, id, title)) in entries.iter().enumerate() {
        if index > 0 {
            if *level > previous {
                result.push_str("<ul>\n");
                open += 1;
            } else if *level == previous {
                result.push_str("</li>\n");
            } else {
                for _ in *level..=previous {
                    result.push_str("</li>\n</ul>\n");
                    open -= 1;
                }
                result.push_str("</li>\n");
            }
        }
        result.push_str(&format!("<li><a href=\"#{id}\">{}</a>", html_escape(title)));
        previous = *level;
    }
    result.push_str("</li>\n");
    while open > 0 {
        result.push_str("</ul>\n");
        open -= 1;
    }
    result.push_str("</div>");
    Some(result)
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_asset_shortcuts() {
        assert_eq!(
            rewrite_asset_tokens("{{ image.webp }} {{ page.html }}", 1),
            "{{ 'image.webp' | static(1) }} {{ 'page.html' | url(1) }}"
        );
    }

    #[test]
    fn leaves_normal_jinja_and_rewrites_multiple_assets() {
        assert_eq!(
            rewrite_asset_tokens("{{ title }} {{ a.png }} {{ b.css }}", 0),
            "{{ title }} {{ 'a.png' | static(0) }} {{ 'b.css' | static(0) }}"
        );
    }

    #[test]
    fn leaves_html_namespace_attributes_for_jinja_evaluation() {
        assert_eq!(
            rewrite_asset_tokens("{{ meet_brooklin.html }}", 0),
            "{{ meet_brooklin.html }}"
        );
    }

    #[test]
    fn protects_attr_list_and_heading_id_markers() {
        let result = preprocess("# Heading {#custom}\n\n{: .wide}", 0);
        assert!(result.contains(HEADING_ID_START));
        assert!(result.contains(INLINE_ATTR_START));
        assert!(restore_protected(&result).contains(INLINE_ATTR_START));
    }

    fn render_fixture(source: &str) -> String {
        let protected = preprocess(source, 0);
        let rendered = restore_protected(&protected);
        postprocess(comrak::markdown_to_html(&rendered, &options()), &rendered)
    }

    #[test]
    fn renders_image_attribute_lists() {
        let html = render_fixture("![fibromyalgie](fibromyalgie.webp){#3 loading=\"lazy\"}");
        assert!(html.contains("<img"));
        assert!(html.contains("id=\"3\""));
        assert!(html.contains("loading=\"lazy\""));
        assert!(!html.contains("/ id=\"3\""));
        assert!(!html.contains(INLINE_ATTR_START));
    }

    #[test]
    fn renders_class_and_key_value_attributes_on_links() {
        let html = render_fixture("[example](target){.example data-kind=\"test\"}");
        assert!(html.contains("class=\"example\""));
        assert!(html.contains("data-kind=\"test\""));
    }

    #[test]
    fn preserves_attribute_looking_text_in_code() {
        let html = render_fixture(
            "`![image](x){#not-an-attribute}`\n\n```\n![image](x){#also-literal}\n```",
        );
        assert!(html.contains("{#not-an-attribute}"));
        assert!(html.contains("{#also-literal}"));
        assert!(!html.contains(INLINE_ATTR_START));
    }

    #[test]
    fn preserves_malformed_attribute_lists() {
        let html = render_fixture("![image](x){#}");
        assert!(html.contains("{#}"));
        assert!(!html.contains(INLINE_ATTR_START));
    }

    #[test]
    fn renders_paragraph_attribute_lists() {
        let html = render_fixture("A paragraph.\n{: .wide}");
        assert!(html.contains("<p class=\"wide\">A paragraph.\n</p>"));
        assert!(!html.contains(INLINE_ATTR_START));
    }

    #[test]
    fn supports_wpm_escaped_attribute_quotes() {
        let html = render_fixture(r#"![image](x){#3 loading=\"lazy\"}"#);
        assert!(html.contains("id=\"3\""));
        assert!(html.contains("loading=\"lazy\""));
    }

    #[test]
    fn renders_python_attribute_list_colon_form_on_images() {
        let html = render_fixture(r#"![image](x){: #3 loading=\"lazy\" }"#);
        assert!(html.contains("id=\"3\""));
        assert!(html.contains("loading=\"lazy\""));
        assert!(!html.contains("{:"));
    }

    #[test]
    fn keeps_heading_attribute_handling_intact() {
        let html = render_fixture("## Heading {: #example .wide}");
        assert!(
            html.contains("<h2 id=\"example\" class=\"wide\">")
                || html.contains("<h2 class=\"wide\" id=\"example\">")
        );
    }

    #[test]
    fn slugifies_python_markdown_heading_text_and_deduplicates() {
        let mut used = HashSet::new();
        assert_eq!(
            slugify("Café & crème brûlée", &mut used),
            "cafe-creme-brulee"
        );
        assert_eq!(
            slugify("Café & crème brûlée", &mut used),
            "cafe-creme-brulee_1"
        );
        assert_eq!(slugify("!!!", &mut used), "_1");
    }

    #[test]
    fn extracts_explicit_heading_ids_from_both_attribute_forms() {
        let ids = explicit_heading_ids("# One {#first}\n## Two {: #second .wide}");
        assert_eq!(ids, vec![Some("first".into()), Some("second".into())]);
    }

    #[test]
    fn builds_nested_python_style_toc() {
        let html = concat!(
            "<h1 id=\"one\"><a class=\"toclink\" href=\"#one\">One</a>",
            "<a class=\"headerlink\" href=\"#one\" title=\"Permanent link\">&para;</a></h1>",
            "<h2 id=\"two\"><a class=\"toclink\" href=\"#two\">Two</a>",
            "<a class=\"headerlink\" href=\"#two\" title=\"Permanent link\">&para;</a></h2>"
        );
        let toc = toc_from_html(html).expect("headings produce a toc");
        assert!(toc.contains("href=\"#one\""));
        assert!(toc.contains("<ul>\n<li><a href=\"#two\">Two</a>"));
    }
}
