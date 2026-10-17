use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    Markdown,
    Html,
}

pub fn classify(path: &Path) -> Option<SourceType> {
    let name = path.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".md") || name.ends_with(".md.j2") || name.ends_with(".jmd") {
        Some(SourceType::Markdown)
    } else if name.ends_with(".html") || name.ends_with(".htm") {
        Some(SourceType::Html)
    } else {
        None
    }
}

/// Maps a template-relative source path to the pretty output path used by the
/// Python renderer. This function has no filesystem side effects.
pub fn source_to_output_path(source: &Path) -> Option<std::path::PathBuf> {
    let mut path = source.to_path_buf();
    if path.strip_prefix("templates").is_ok() {
        path = path.strip_prefix("templates").unwrap().to_path_buf();
    }
    if path.strip_prefix("root").is_ok() {
        path = path.strip_prefix("root").unwrap().to_path_buf();
    }
    let mut name = path.file_name()?.to_string_lossy().into_owned();
    loop {
        let lower = name.to_ascii_lowercase();
        let suffix = [".html", ".htm", ".md", ".jmd", ".j2", ".jinja2"]
            .iter()
            .find(|s| lower.ends_with(**s));
        if let Some(suffix) = suffix {
            name.truncate(name.len() - suffix.len());
        } else {
            break;
        }
    }
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    if name.eq_ignore_ascii_case("index") && parent.as_os_str().is_empty() {
        Some(Path::new("index.html").to_path_buf())
    } else {
        Some(parent.join(name).join("index.html"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies_current_extensions() {
        assert_eq!(classify(Path::new("a.md")), Some(SourceType::Markdown));
        assert_eq!(classify(Path::new("a.md.j2")), Some(SourceType::Markdown));
        assert_eq!(classify(Path::new("a.jmd")), Some(SourceType::Markdown));
        assert_eq!(classify(Path::new("a.html")), Some(SourceType::Html));
        assert_eq!(classify(Path::new("a.htm")), Some(SourceType::Html));
    }
    #[test]
    fn maps_pretty_paths() {
        assert_eq!(
            source_to_output_path(Path::new("index.html")).unwrap(),
            Path::new("index.html")
        );
        assert_eq!(
            source_to_output_path(Path::new("about.html")).unwrap(),
            Path::new("about/index.html")
        );
        assert_eq!(
            source_to_output_path(Path::new("root/about.md")).unwrap(),
            Path::new("about/index.html")
        );
        assert_eq!(
            source_to_output_path(Path::new("templates/about.htm")).unwrap(),
            Path::new("about/index.html")
        );
        assert_eq!(
            source_to_output_path(Path::new("blog/post.md.j2")).unwrap(),
            Path::new("blog/post/index.html")
        );
    }
}
