use crate::{
    graph::SiteGraph,
    manifest::{BuildState, RenderManifest},
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildPlan {
    pub pages_to_render: Vec<String>,
    pub derived_artifacts_to_rebuild: Vec<String>,
    pub stale_candidates: Vec<String>,
}

pub fn plan(
    graph: &SiteGraph,
    previous: Option<&BuildState>,
    output_root: &Path,
    global_render_fingerprint: &str,
) -> BuildPlan {
    let global_changed = previous
        .map(|state| state.global_render_fingerprint != global_render_fingerprint)
        .unwrap_or(true);
    let mut changed_sections = BTreeSet::new();
    let mut pages_to_render = Vec::new();
    for page in graph.logical_pages() {
        let unchanged = !global_changed
            && previous
                .and_then(|state| state.pages.get(&page.source))
                .is_some_and(|old| {
                    old.fingerprint == page.fingerprint && output_root.join(&old.output).is_file()
                });
        if !unchanged {
            pages_to_render.push(page.source.clone());
            changed_sections.insert(page.sub_dir.clone());
        }
    }
    for page in graph.logical_pages() {
        if page.sub_dir.is_empty()
            && changed_sections.contains(&page.page_name)
            && !pages_to_render.contains(&page.source)
        {
            pages_to_render.push(page.source.clone());
        }
    }
    pages_to_render.sort();
    let current_sources = graph
        .logical_pages()
        .into_iter()
        .map(|page| (page.source.as_str(), page.output.as_str()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let stale_candidates = previous
        .map(|state| {
            state
                .source_to_output
                .iter()
                .filter(|(source, _)| !current_sources.contains_key(source.as_str()))
                .map(|(_, output)| output.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for source in previous
        .map(|state| {
            state
                .source_to_output
                .keys()
                .filter(|source| !current_sources.contains_key(source.as_str()))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
    {
        if let Some(section) = Path::new(&source).components().next() {
            changed_sections.insert(section.as_os_str().to_string_lossy().into_owned());
        }
    }
    for page in graph.logical_pages() {
        if page.sub_dir.is_empty()
            && changed_sections.contains(&page.page_name)
            && !pages_to_render.contains(&page.source)
        {
            pages_to_render.push(page.source.clone());
        }
    }
    pages_to_render.sort();
    let mut derived_artifacts_to_rebuild = Vec::new();
    if !pages_to_render.is_empty() || !stale_candidates.is_empty() || global_changed {
        derived_artifacts_to_rebuild.extend(
            ["page_references.json", "rss.xml", "sitemap.xml"]
                .into_iter()
                .map(String::from),
        );
    }
    if global_changed {
        derived_artifacts_to_rebuild.push("robots.txt".into());
    }
    BuildPlan {
        pages_to_render,
        derived_artifacts_to_rebuild,
        stale_candidates,
    }
}

pub fn global_render_fingerprint(
    site_root: &Path,
    config: &serde_json::Value,
) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(b"jnja-global-render-v2\0");
    hasher.update(serde_json::to_vec(config).map_err(|error| error.to_string())?);
    let template_root = site_root.join("templates");
    for directory in ["layouts", "macros", "sections"] {
        let path = template_root.join(directory);
        if path.is_dir() {
            let mut files = Vec::new();
            collect_files(&path, &mut files).map_err(|error| error.to_string())?;
            files.sort();
            for file in files {
                let relative = file
                    .strip_prefix(site_root)
                    .map_err(|error| error.to_string())?;
                hasher.update(relative.to_string_lossy().as_bytes());
                hasher.update(fs::read(&file).map_err(|error| error.to_string())?);
            }
        }
    }
    let base_template = template_root.join("base.html");
    if base_template.is_file() {
        hasher.update(b"templates/base.html");
        hasher.update(fs::read(base_template).map_err(|error| error.to_string())?);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn collect_files(path: &Path, files: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(path)? {
        let child = entry?.path();
        if child.is_dir() {
            collect_files(&child, files)?;
        } else if child.is_file() {
            files.push(child);
        }
    }
    Ok(())
}

pub fn state_from_build(
    graph: &SiteGraph,
    manifest: &RenderManifest,
    previous: Option<&BuildState>,
    global_render_fingerprint: String,
    static_files: std::collections::BTreeMap<String, crate::manifest::StaticFileState>,
) -> BuildState {
    let mut state = BuildState::new();
    state.global_render_fingerprint = global_render_fingerprint;
    state.static_files = static_files;
    for page in graph.logical_pages() {
        state.pages.insert(
            page.source.clone(),
            crate::manifest::StatePage {
                source: page.source.clone(),
                output: page.output.clone(),
                fingerprint: page.fingerprint.clone(),
            },
        );
        state
            .source_to_output
            .insert(page.source.clone(), page.output.clone());
        state
            .dependencies
            .insert(page.source.clone(), vec![page.output.clone()]);
    }
    if let Some(previous) = previous {
        state.artifact_hashes = previous.artifact_hashes.clone();
    }
    for artifact in &manifest.generated {
        state
            .artifact_hashes
            .insert(artifact.path.clone(), artifact.sha256.clone());
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{graph::PageNode, manifest::StatePage, source::SourceType};
    use serde_json::Map;
    fn page(source: &str, output: &str, sub_dir: &str, name: &str, fingerprint: &str) -> PageNode {
        PageNode {
            source: source.into(),
            kind: SourceType::Html,
            output: output.into(),
            route: format!("/{name}/"),
            depth: 1,
            page_name: name.into(),
            sub_dir: sub_dir.into(),
            metadata: Map::new(),
            fingerprint: fingerprint.into(),
        }
    }
    #[test]
    fn changed_child_rebuilds_section_overview() {
        let graph = SiteGraph {
            pages: vec![
                page(
                    "blog/post.html",
                    "blog/post/index.html",
                    "blog",
                    "post",
                    "new",
                ),
                page("blog.html", "blog/index.html", "", "blog", "old"),
            ],
        };
        let mut state = BuildState::new();
        state.global_render_fingerprint = "same".into();
        state.pages.insert(
            "blog/post.html".into(),
            StatePage {
                source: "blog/post.html".into(),
                output: "blog/post/index.html".into(),
                fingerprint: "old".into(),
            },
        );
        state.pages.insert(
            "blog.html".into(),
            StatePage {
                source: "blog.html".into(),
                output: "blog/index.html".into(),
                fingerprint: "old".into(),
            },
        );
        state
            .source_to_output
            .insert("blog/post.html".into(), "blog/post/index.html".into());
        state
            .source_to_output
            .insert("blog.html".into(), "blog/index.html".into());
        let plan = plan(&graph, Some(&state), Path::new("/tmp"), "same");
        assert!(plan.pages_to_render.contains(&"blog/post.html".into()));
        assert!(plan.pages_to_render.contains(&"blog.html".into()));
    }

    #[test]
    fn new_child_rebuilds_section_overview_and_derived_artifacts() {
        let graph = SiteGraph {
            pages: vec![
                page("blog/new.html", "blog/new/index.html", "blog", "new", "new"),
                page("blog.html", "blog/index.html", "", "blog", "same"),
            ],
        };
        let mut state = BuildState::new();
        state.global_render_fingerprint = "same".into();
        state.pages.insert(
            "blog.html".into(),
            StatePage {
                source: "blog.html".into(),
                output: "blog/index.html".into(),
                fingerprint: "same".into(),
            },
        );
        state
            .source_to_output
            .insert("blog.html".into(), "blog/index.html".into());
        let plan = plan(&graph, Some(&state), Path::new("/tmp"), "same");
        assert!(plan.pages_to_render.contains(&"blog/new.html".into()));
        assert!(plan.pages_to_render.contains(&"blog.html".into()));
        assert!(plan
            .derived_artifacts_to_rebuild
            .contains(&"page_references.json".into()));
        assert!(plan
            .derived_artifacts_to_rebuild
            .contains(&"rss.xml".into()));
    }
    #[test]
    fn removed_pages_become_stale_candidates() {
        let graph = SiteGraph {
            pages: vec![page("index.html", "index.html", "", "index", "same")],
        };
        let mut state = BuildState::new();
        state.global_render_fingerprint = "same".into();
        state
            .source_to_output
            .insert("old.md".into(), "old/index.html".into());
        state.pages.insert(
            "index.html".into(),
            StatePage {
                source: "index.html".into(),
                output: "index.html".into(),
                fingerprint: "same".into(),
            },
        );
        let plan = plan(&graph, Some(&state), Path::new("/tmp"), "same");
        assert_eq!(plan.stale_candidates, vec!["old/index.html"]);
        assert!(plan
            .derived_artifacts_to_rebuild
            .contains(&"rss.xml".into()));
    }

    #[test]
    fn removed_child_rebuilds_existing_section_overview() {
        let graph = SiteGraph {
            pages: vec![page("blog.html", "blog/index.html", "", "blog", "same")],
        };
        let mut state = BuildState::new();
        state.global_render_fingerprint = "same".into();
        state
            .source_to_output
            .insert("blog/old.md".into(), "blog/old/index.html".into());
        state.pages.insert(
            "blog.html".into(),
            StatePage {
                source: "blog.html".into(),
                output: "blog/index.html".into(),
                fingerprint: "same".into(),
            },
        );
        let plan = plan(&graph, Some(&state), Path::new("/tmp"), "same");
        assert_eq!(plan.pages_to_render, vec!["blog.html"]);
        assert_eq!(plan.stale_candidates, vec!["blog/old/index.html"]);
    }
    #[test]
    fn global_change_rebuilds_all_pages_and_robots() {
        let graph = SiteGraph {
            pages: vec![
                page("a.html", "a/index.html", "", "a", "same"),
                page("b.html", "b/index.html", "", "b", "same"),
            ],
        };
        let mut state = BuildState::new();
        state.global_render_fingerprint = "old".into();
        for source in ["a.html", "b.html"] {
            state.pages.insert(
                source.into(),
                StatePage {
                    source: source.into(),
                    output: format!("{source}/index.html"),
                    fingerprint: "same".into(),
                },
            );
        }
        let plan = plan(&graph, Some(&state), Path::new("/tmp"), "new");
        assert_eq!(plan.pages_to_render.len(), 2);
        assert!(plan
            .derived_artifacts_to_rebuild
            .contains(&"robots.txt".into()));
    }

    #[test]
    fn state_keeps_unchanged_pages_across_incremental_runs() {
        let root = std::env::temp_dir().join(format!(
            "jnja-state-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        for output in ["a/index.html", "b/index.html", "c/index.html"] {
            let path = root.join(output);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "page").unwrap();
        }
        let graph = SiteGraph {
            pages: vec![
                page("a.html", "a/index.html", "", "a", "a"),
                page("b.html", "b/index.html", "", "b", "b-new"),
                page("c.html", "c/index.html", "", "c", "c"),
            ],
        };
        let mut state = BuildState::new();
        state.global_render_fingerprint = "same".into();
        for (source, output) in [
            ("a.html", "a/index.html"),
            ("b.html", "b/index.html"),
            ("c.html", "c/index.html"),
        ] {
            state.pages.insert(
                source.into(),
                StatePage {
                    source: source.into(),
                    output: output.into(),
                    fingerprint: source.strip_suffix(".html").unwrap().into(),
                },
            );
            state.source_to_output.insert(source.into(), output.into());
        }
        let first = plan(&graph, Some(&state), &root, "same");
        assert_eq!(first.pages_to_render, vec!["b.html"]);
        let current = state_from_build(
            &graph,
            &RenderManifest::new(crate::request::RenderMode::Incremental),
            Some(&state),
            "same".into(),
            std::collections::BTreeMap::new(),
        );
        let second = plan(&graph, Some(&current), &root, "same");
        assert!(second.pages_to_render.is_empty());
        assert_eq!(current.pages.len(), 3);
        std::fs::remove_dir_all(root).unwrap();
    }
}
