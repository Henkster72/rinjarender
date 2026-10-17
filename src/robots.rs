pub fn generate(base_url: &str, allow_paths: &[String]) -> String {
    let mut out = String::from("User-agent: OAI-SearchBot\nAllow: /\n\nUser-agent: *\nAllow: /\n");
    for path in allow_paths {
        let path = path.trim_matches('/');
        if !path.is_empty() {
            out.push_str(&format!("Allow: /{path}\n"));
        }
    }
    out.push_str(&format!(
        "Sitemap: {}/sitemap.xml\n",
        base_url.trim_end_matches('/')
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::generate;

    #[test]
    fn renders_configured_allow_paths_and_normalized_sitemap() {
        let output = generate(
            "https://example.test/",
            &[
                "page_references.json".into(),
                "/static/tabsdata.json".into(),
            ],
        );
        assert!(output.contains("Allow: /page_references.json"));
        assert!(output.contains("Allow: /static/tabsdata.json"));
        assert!(output.contains("Sitemap: https://example.test/sitemap.xml"));
    }
}
