use crate::source::{self, SourceType};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    pub relative_path: String,
    pub absolute_path: PathBuf,
    pub kind: SourceType,
}

pub fn discover(template_root: &Path) -> Result<Vec<SourceEntry>, Box<dyn std::error::Error>> {
    let mut entries = Vec::new();
    visit(template_root, template_root, &mut entries)?;
    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(entries)
}

fn visit(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<SourceEntry>,
) -> Result<(), Box<dyn std::error::Error>> {
    for item in fs::read_dir(directory)? {
        let path = item?.path();
        if path.is_dir() {
            let relative = path.strip_prefix(root)?;
            if relative.components().any(|part| {
                matches!(
                    part.as_os_str().to_str(),
                    Some("layouts" | "macros" | "sections")
                )
            }) {
                continue;
            }
            visit(root, &path, entries)?;
            continue;
        }
        let relative = path.strip_prefix(root)?;
        if ignored(relative) {
            continue;
        }
        let Some(kind) = source::classify(relative) else {
            continue;
        };
        entries.push(SourceEntry {
            relative_path: relative.to_string_lossy().replace('\\', "/"),
            absolute_path: path,
            kind,
        });
    }
    Ok(())
}

fn ignored(relative: &Path) -> bool {
    let Some(name) = relative.file_name().and_then(|v| v.to_str()) else {
        return true;
    };
    if relative.components().any(|part| {
        matches!(
            part.as_os_str().to_str(),
            Some("layouts" | "macros" | "sections")
        )
    }) {
        return true;
    }
    name == "base.html"
        || name.starts_with("temp_")
        || name.starts_with("macro_")
        || name.starts_with("section_")
        || name.contains("_card") && name.ends_with(".htm")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn excludes_support_templates_and_keeps_root_pages() {
        let root = std::env::temp_dir().join(format!(
            "jnja-discovery-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("macros")).unwrap();
        fs::create_dir_all(root.join("root")).unwrap();
        fs::write(root.join("base.html"), "").unwrap();
        fs::write(root.join("macro_x.html"), "").unwrap();
        fs::write(root.join("macros/m.html"), "").unwrap();
        fs::write(root.join("root/about.md"), "").unwrap();
        fs::write(root.join("blog.md.j2"), "").unwrap();
        let found = discover(&root).unwrap();
        assert_eq!(
            found
                .iter()
                .map(|x| x.relative_path.as_str())
                .collect::<Vec<_>>(),
            vec!["blog.md.j2", "root/about.md"]
        );
        fs::remove_dir_all(root).unwrap();
    }
}
