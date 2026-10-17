/// Join an absolute base URL and a route without duplicating the boundary slash.
pub fn join_base_path(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let path = path.trim_matches('/');
    if path.is_empty() {
        format!("{base}/")
    } else {
        format!("{base}/{path}/")
    }
}

/// Return the public route for a page output path. Page identity segments use
/// hyphens in public routes, while the output mapping remains filesystem-based.
pub fn route_for_output(output: &str) -> String {
    if output == "index.html" {
        return "/".into();
    }
    let route = output.trim_end_matches("/index.html");
    format!("/{}/", canonical_page_segments(route))
}

/// Resolve a page reference for the renderer's URL filter. Explicit URLs and
/// literal paths remain untouched; page identifiers use the canonical route.
pub fn page_route(path: &str, depth: i64) -> String {
    let normalized = path.trim().replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with("/")
        || normalized.starts_with('#')
        || normalized.starts_with("//")
        || normalized.contains("://")
        || normalized.starts_with("mailto:")
        || normalized.starts_with("tel:")
        || normalized.contains('?')
        || (normalized.contains('/')
            && !normalized.ends_with(".html")
            && !normalized.ends_with(".htm"))
    {
        return normalized;
    }
    let page = normalized
        .strip_suffix(".html")
        .or_else(|| normalized.strip_suffix(".htm"))
        .unwrap_or(&normalized);
    let route = format!("{}/", canonical_page_segments(page));
    format!("{}{}", "../".repeat(depth.max(0) as usize), route)
}

fn canonical_page_segments(path: &str) -> String {
    path.split('/')
        .map(|segment| segment.replace('_', "-"))
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::{join_base_path, page_route, route_for_output};

    #[test]
    fn joins_trailing_base_and_leading_route_once() {
        assert_eq!(
            join_base_path("https://example.test/", "/nieuws/item/"),
            "https://example.test/nieuws/item/"
        );
    }

    #[test]
    fn joins_base_without_trailing_slash() {
        assert_eq!(
            join_base_path("https://example.test", "/nieuws/item/"),
            "https://example.test/nieuws/item/"
        );
    }

    #[test]
    fn preserves_root_url() {
        assert_eq!(
            join_base_path("https://example.test/", "/"),
            "https://example.test/"
        );
    }

    #[test]
    fn page_routes_canonicalize_identifiers_only() {
        assert_eq!(page_route("meet_brooklin", 0), "meet-brooklin/");
        assert_eq!(page_route("meet_brooklin.html", 0), "meet-brooklin/");
        assert_eq!(
            page_route("blog/meet_brooklin.html", 1),
            "../blog/meet-brooklin/"
        );
        assert_eq!(page_route("/some_custom/path/", 0), "/some_custom/path/");
        assert_eq!(
            page_route("https://example.test/some_custom/", 0),
            "https://example.test/some_custom/"
        );
        assert_eq!(page_route("#some_custom", 0), "#some_custom");
        assert_eq!(
            page_route("mailto:a_b@example.test", 0),
            "mailto:a_b@example.test"
        );
        assert_eq!(page_route("tel:+31123456789", 0), "tel:+31123456789");
    }

    #[test]
    fn output_and_navigation_use_the_same_page_route() {
        assert_eq!(
            route_for_output("meet-brooklin/index.html"),
            "/meet-brooklin/"
        );
        assert_eq!(page_route("meet_brooklin.html", 0), "meet-brooklin/");
    }
}
