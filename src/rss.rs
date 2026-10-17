use crate::{graph::SiteGraph, url::join_base_path};
use serde_json::Value;

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn date_parts(value: &str) -> Option<(i32, u32, u32)> {
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
            parts[0].parse().ok()?,
            parts[1].as_str(),
            parts[2].parse().ok()?,
        )
    } else {
        (
            parts[1].parse().ok()?,
            parts[0].as_str(),
            parts[2].parse().ok()?,
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
    Some((year, month, day))
}

fn rss_date(value: &str) -> Option<String> {
    let (year, month, day) = date_parts(value)?;
    let (y, m) = if month < 3 {
        (year - 1, month + 12)
    } else {
        (year, month)
    };
    let weekday = (day as i32 + (13 * (m as i32 + 1)) / 5 + y + y / 4 - y / 100 + y / 400) % 7;
    let weekday = ["Sat", "Sun", "Mon", "Tue", "Wed", "Thu", "Fri"][weekday as usize];
    let month_name = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][month as usize - 1];
    Some(format!(
        "{weekday}, {day:02} {month_name} {year} 00:00:00 +0000"
    ))
}

pub fn generate(graph: &SiteGraph, base_url: &str, config: &Value) -> String {
    let rss = config.get("rss").and_then(Value::as_object);
    let title = rss
        .and_then(|v| v.get("title"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            config
                .pointer("/site/name")
                .and_then(Value::as_str)
                .map(|value| format!("{value} RSS"))
        })
        .unwrap_or_else(|| "Site RSS".into());
    let description = rss
        .and_then(|v| v.get("description"))
        .and_then(Value::as_str)
        .or_else(|| config.pointer("/seo/description").and_then(Value::as_str))
        .unwrap_or("Latest pages");
    let language = rss
        .and_then(|v| v.get("language"))
        .and_then(Value::as_str)
        .or_else(|| config.pointer("/site/language").and_then(Value::as_str))
        .unwrap_or("nl");
    let sub_dir = rss
        .and_then(|v| v.get("sub_dir"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let mut pages = graph
        .logical_pages()
        .into_iter()
        .filter(|page| {
            !page.sub_dir.is_empty()
                && sub_dir.is_none_or(|configured| page.sub_dir == configured)
                && page
                    .metadata
                    .get("post_date")
                    .or_else(|| page.metadata.get("date"))
                    .and_then(Value::as_str)
                    .is_some_and(|date| date_parts(date).is_some())
        })
        .collect::<Vec<_>>();
    pages.sort_by(|a, b| {
        let date = |page: &crate::graph::PageNode| {
            page.metadata
                .get("post_date")
                .or_else(|| page.metadata.get("date"))
                .and_then(Value::as_str)
                .and_then(date_parts)
        };
        date(b).cmp(&date(a)).then_with(|| a.source.cmp(&b.source))
    });
    let last_build = pages.first().and_then(|page| {
        page.metadata
            .get("post_date")
            .or_else(|| page.metadata.get("date"))
            .and_then(Value::as_str)
            .and_then(rss_date)
    });
    let channel_url = join_base_path(
        base_url,
        &sub_dir.map_or_else(|| "/".into(), |value| format!("/{value}/")),
    );
    let mut out = format!("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<rss version=\"2.0\"><channel><title>{}</title><link>{}</link><description>{}</description><language>{}</language>", escape(&title), escape(&channel_url), escape(description), escape(language));
    if let Some(last_build) = last_build {
        out.push_str(&format!(
            "<lastBuildDate>{}</lastBuildDate>",
            escape(&last_build)
        ));
    }
    for page in pages {
        let m = &page.metadata;
        let url = join_base_path(base_url, &page.route);
        let page_title = m
            .get("page_title")
            .or_else(|| m.get("title"))
            .and_then(Value::as_str)
            .unwrap_or(&page.page_name);
        let subtitle = m
            .get("page_subtitle")
            .or_else(|| m.get("subtitle"))
            .and_then(Value::as_str)
            .unwrap_or("");
        out.push_str(&format!(
            "<item><title>{}</title><link>{}</link><guid>{}</guid><description>{}</description>",
            escape(page_title),
            escape(&url),
            escape(&url),
            escape(subtitle)
        ));
        if let Some(date) = m
            .get("post_date")
            .or_else(|| m.get("date"))
            .and_then(Value::as_str)
            .and_then(rss_date)
        {
            out.push_str(&format!("<pubDate>{}</pubDate>", escape(&date)));
        }
        if let (Some(picture), Some(mime)) = (
            m.get("page_picture_url")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty()),
            m.get("page_picture_mime_type")
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty()),
        ) {
            out.push_str(&format!(
                "<enclosure url=\"{}\" type=\"{}\" />",
                escape(picture),
                escape(mime)
            ));
        }
        out.push_str("</item>");
    }
    out.push_str("</channel></rss>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::generate;
    use crate::graph::{PageNode, SiteGraph};
    use crate::source::SourceType;
    use serde_json::json;
    fn page(name: &str, date: &str) -> PageNode {
        PageNode {
            source: format!("blog/{name}.html"),
            kind: SourceType::Html,
            output: format!("blog/{name}/index.html"),
            route: format!("/blog/{name}/"),
            depth: 2,
            page_name: name.into(),
            sub_dir: "blog".into(),
            metadata: json!({"page_title": name, "post_date": date})
                .as_object()
                .cloned()
                .unwrap(),
            fingerprint: String::new(),
        }
    }
    #[test]
    fn uses_configured_values_and_date_order() {
        let graph = SiteGraph {
            pages: vec![page("old", "dec 17, 2019"), page("new", "sep 10, 2024")],
        };
        let config = json!({"site":{"language":"en"},"rss":{"title":"Flowing Numbers Blog","description":"Configured","language":"en","sub_dir":"blog"}});
        let output = generate(&graph, "https://example.test/", &config);
        assert!(output.contains("<title>Flowing Numbers Blog</title>"));
        assert!(output.contains("<language>en</language>"));
        assert!(
            output.find("<title>new</title>").unwrap() < output.find("<title>old</title>").unwrap()
        );
        assert!(output.contains("<lastBuildDate>Tue, 10 Sep 2024"));
    }

    #[test]
    fn supports_wch_style_all_dated_content_without_rss_section() {
        let graph = SiteGraph {
            pages: vec![
                PageNode {
                    source: "nieuws/article.html".into(),
                    kind: SourceType::Html,
                    output: "nieuws/article/index.html".into(),
                    route: "/nieuws/article/".into(),
                    depth: 2,
                    page_name: "article".into(),
                    sub_dir: "nieuws".into(),
                    metadata: json!({
                        "page_title": "Example article",
                        "page_subtitle": "Article summary",
                        "post_date": "14 oktober 2026"
                    })
                    .as_object()
                    .cloned()
                    .unwrap(),
                    fingerprint: String::new(),
                },
                PageNode {
                    source: "partners/partner.html".into(),
                    kind: SourceType::Html,
                    output: "partners/partner/index.html".into(),
                    route: "/partners/partner/".into(),
                    depth: 2,
                    page_name: "partner".into(),
                    sub_dir: "partners".into(),
                    metadata: json!({
                        "page_title": "Partner",
                        "post_date": "6 september 2026"
                    })
                    .as_object()
                    .cloned()
                    .unwrap(),
                    fingerprint: String::new(),
                },
            ],
        };
        let config = json!({
            "site": {"name": "Example Site", "language": "nl"},
            "seo": {"description": "Canonical example description"}
        });
        let output = generate(&graph, "https://www.wchnederland.nl/", &config);
        assert!(output.contains("<title>Example Site RSS</title>"));
        assert!(output.contains("<description>Canonical example description</description>"));
        assert!(output.contains("<language>nl</language>"));
        assert!(output.contains("https://www.wchnederland.nl/nieuws/article/"));
        assert!(output.contains("https://www.wchnederland.nl/partners/partner/"));
        assert!(output.contains("<lastBuildDate>Wed, 14 Oct 2026"));
        assert!(!output.contains("1970"));
    }
}
