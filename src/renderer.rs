use crate::{
    context, discovery, frontmatter, graph, incremental,
    manifest::{BuildState, PageRecord, RenderManifest},
    markdown, metadata, output,
    request::{RenderEnvironment, RenderRequest},
    robots, rss, seo, site, sitemap,
    source::{self, SourceType},
    static_sync,
    template::TemplateEngine,
};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
};

fn render_card_sets(
    engine: &TemplateEngine,
    base_context: &context::ContextMap,
    config: &serde_json::Value,
    index: &graph::RenderIndex<'_>,
) -> HashMap<String, minijinja::value::Value> {
    let default_template = config
        .get("defaults")
        .and_then(|defaults| defaults.get("page_card_template"))
        .and_then(serde_json::Value::as_str);
    index
        .cards_by_section
        .iter()
        .map(|(section, candidates)| {
            let cards = candidates
                .iter()
                .map(|candidate| {
                    let template = candidate
                        .metadata
                        .get("page_card_template")
                        .and_then(serde_json::Value::as_str)
                        .filter(|value| !value.is_empty())
                        .or(default_template)
                        .unwrap_or("generic_card.htm");
                    let mut card_context = base_context.clone();
                    card_context.extend(candidate.metadata.clone());
                    card_context.insert(
                        "page_name".into(),
                        serde_json::Value::String(candidate.page_name.clone()),
                    );
                    card_context.insert(
                        "sub_dir".into(),
                        serde_json::Value::String(candidate.sub_dir.clone()),
                    );
                    engine
                        .render_named(template, &card_context)
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>();
            (
                section.clone(),
                minijinja::value::Value::from_serialize(cards),
            )
        })
        .collect()
}

pub fn render(request: &RenderRequest) -> Result<RenderManifest, Box<dyn std::error::Error>> {
    request.validate()?;
    let site = site::load(request)?;
    let mut manifest = RenderManifest::new(request.mode.clone());
    let explicit_context = context::load_json(request.context_path().as_deref())?;
    let base_context = context::build_base_context(
        &site.config,
        explicit_context,
        &site.root,
        &request.output_root,
        matches!(request.environment, RenderEnvironment::Production),
    );
    let mut image_cache = metadata::ImageCache::load(&request.image_cache_path());
    let entries = discovery::discover(&site.templates)?;
    let base_url = base_context
        .get("base_url")
        .and_then(serde_json::Value::as_str);
    let graph = graph::SiteGraph::discover(
        &entries,
        &site.root,
        &site.config,
        base_url,
        &mut image_cache,
    )?;
    let render_index = graph.render_index();
    manifest.page_references = render_index.page_references.clone();
    let logical_pages = render_index.logical_pages.clone();
    manifest.source_to_output = logical_pages
        .iter()
        .map(|page| (page.source.clone(), page.output.clone()))
        .collect();
    let global_fingerprint = incremental::global_render_fingerprint(&site.root, &site.config)?;
    let previous = if matches!(request.mode, crate::request::RenderMode::Incremental) {
        request
            .state_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str::<BuildState>(&text).ok())
    } else {
        None
    };
    let build_plan = if matches!(request.mode, crate::request::RenderMode::Incremental) {
        Some(incremental::plan(
            &graph,
            previous.as_ref(),
            &request.output_root,
            &global_fingerprint,
        ))
    } else {
        None
    };
    let selected = if matches!(request.mode, crate::request::RenderMode::Selected) {
        request.templates.clone()
    } else if matches!(request.mode, crate::request::RenderMode::Incremental) {
        build_plan
            .as_ref()
            .expect("incremental build plan")
            .pages_to_render
            .clone()
    } else {
        logical_pages
            .iter()
            .map(|page| page.source.clone())
            .collect()
    };
    let engine = TemplateEngine::new_site(&site.templates);
    let card_sets = render_card_sets(&engine, &base_context, &site.config, &render_index);
    let mut static_files_for_state = BTreeMap::new();
    for relative in &selected {
        let source_path = site::input_path(&site, relative);
        let source = fs::read_to_string(&source_path)?;
        let source_type = source::classify(&source_path)
            .ok_or_else(|| format!("unsupported source type: {relative}"))?;
        let relative_output = source::source_to_output_path(std::path::Path::new(relative))
            .ok_or_else(|| format!("could not map source path: {relative}"))?;
        let depth = relative_output
            .parent()
            .map(|parent| parent.components().count() as i64)
            .unwrap_or(0);
        let page = render_index.pages_by_source.get(relative.as_str()).copied();
        let page_references_value = page
            .filter(|page| page.sub_dir.is_empty() && page.page_name != "index")
            .and_then(|page| card_sets.get(&page.page_name))
            .unwrap_or(&render_index.page_references_value);
        let mut page_context = base_context.clone();
        context::add_page_context(
            &mut page_context,
            relative,
            &relative_output,
            depth,
            &site.root,
        );
        if let Some(page) = page {
            page_context.extend(page.metadata.clone());
        }
        if !page_context.contains_key("page_picture") {
            if let Some(default_picture) = site
                .config
                .get("defaults")
                .and_then(|value| value.get("page_picture"))
                .cloned()
            {
                page_context.insert("page_picture".into(), default_picture);
            }
        }
        metadata::enrich_page_context_cached(&mut page_context, &site.root, &mut image_cache);
        let rendered = match source_type {
            SourceType::Html => {
                let source = crate::compatibility::markdown::rewrite_asset_tokens_runtime(&source);
                let source =
                    crate::compatibility::jinja::prepare_template(&source, &site.templates);
                let meta_tags = seo::render_meta_tags_with_references(
                    &site.config,
                    &mut page_context,
                    &render_index.page_references,
                );
                page_context.insert(
                    "meta_tags".into(),
                    serde_json::Value::String(meta_tags.clone()),
                );
                page_context.insert(
                    "schema_tags".into(),
                    serde_json::Value::String(seo::json_ld_tags(&meta_tags)),
                );
                engine.render_inline_value(
                    relative,
                    &source,
                    context::as_layout_template_value(&page_context, page_references_value),
                )?
            }
            SourceType::Markdown => {
                let document = frontmatter::split(&source)?;
                let mut metadata = document.metadata.clone();
                frontmatter::render_string_values(&mut metadata, |value| {
                    Ok(engine.render_inline_value(
                        "frontmatter-value",
                        value,
                        context::as_template_value_with_references(
                            &page_context,
                            page_references_value,
                        ),
                    )?)
                })?;
                frontmatter::normalize_aliases(&mut metadata);
                page_context.extend(metadata.clone());
                metadata::enrich_page_context_cached(
                    &mut page_context,
                    &site.root,
                    &mut image_cache,
                );
                let html = markdown::render_document_with_value(
                    &frontmatter::Document {
                        metadata,
                        body: document.body,
                    },
                    &engine,
                    &page_context,
                    context::as_template_value_with_references(
                        &page_context,
                        page_references_value,
                    ),
                )?;
                if let Some(toc) = crate::compatibility::markdown::toc_from_html(&html) {
                    page_context.insert("markdown_toc".into(), serde_json::Value::String(toc));
                }
                page_context.insert(
                    "markdown_html".into(),
                    serde_json::Value::String(html.clone()),
                );
                page_context.insert("content".into(), serde_json::Value::String(html));
                page_context.insert(
                    "markdown_source".into(),
                    serde_json::Value::String(source.clone()),
                );
                let meta_tags = seo::render_meta_tags_with_references(
                    &site.config,
                    &mut page_context,
                    &render_index.page_references,
                );
                page_context.insert(
                    "meta_tags".into(),
                    serde_json::Value::String(meta_tags.clone()),
                );
                page_context.insert(
                    "schema_tags".into(),
                    serde_json::Value::String(seo::json_ld_tags(&meta_tags)),
                );
                let layout = request
                    .layout
                    .clone()
                    .unwrap_or_else(|| "layouts/markdown_page.html".into());
                engine.render_named_value(
                    &layout,
                    context::as_layout_template_value(&page_context, page_references_value),
                )?
            }
        };
        let mut record = output::write_page(&request.output_root, &relative_output, &rendered)?;
        record.source = Some(relative.clone());
        let record_state = record.state;
        let output_name = relative_output.to_string_lossy().into_owned();
        if !matches!(record_state, crate::manifest::ArtifactState::Unchanged) {
            manifest.changed.push(output_name.clone());
        }
        manifest.generated.push(record);
        manifest.pages.push(PageRecord {
            source: relative.clone(),
            output: output_name,
            changed: !matches!(record_state, crate::manifest::ArtifactState::Unchanged),
            state: record_state,
            fingerprint: render_index
                .pages_by_source
                .get(relative.as_str())
                .map(|page| page.fingerprint.clone())
                .unwrap_or_default(),
            warnings: vec![],
        });
    }
    if !matches!(request.mode, crate::request::RenderMode::Selected) && !request.render_only {
        let static_source = if site.root.join("static").is_dir() {
            site.root.join("static")
        } else {
            site.root.join("output/static")
        };
        let (static_records, static_files) = static_sync::sync_with_state(
            &static_source,
            &request.output_root.join("static"),
            previous.as_ref().map(|state| &state.static_files),
            request.verify_static,
        )?;
        static_files_for_state = static_files;
        manifest.generated.extend(static_records);
        manifest.stale_candidates = static_sync::stale_candidates(
            &static_source,
            &request.output_root.join("static"),
            previous.as_ref().map(|state| &state.static_files),
        )?;
        if let Some(plan) = &build_plan {
            manifest
                .stale_candidates
                .extend(plan.stale_candidates.clone());
            manifest.stale_candidates.sort();
            manifest.stale_candidates.dedup();
        }
        let default_derived = [
            "page_references.json",
            "rss.xml",
            "sitemap.xml",
            "robots.txt",
        ];
        let requested = build_plan
            .as_ref()
            .map(|plan| {
                plan.derived_artifacts_to_rebuild
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| default_derived.to_vec());
        if requested.contains(&"page_references.json") {
            let record = output::write_artifact(
                &request.output_root,
                std::path::Path::new("page_references.json"),
                serde_json::to_string_pretty(&manifest.page_references)?.as_bytes(),
                None,
                "page_references",
            )?;
            if !matches!(record.state, crate::manifest::ArtifactState::Unchanged) {
                manifest.changed.push("page_references.json".into());
            }
            manifest.generated.push(record);
        }
        if let Some(base_url) = base_url {
            let mut artifacts = Vec::new();
            if requested.contains(&"rss.xml") {
                artifacts.push((
                    "rss.xml",
                    rss::generate(&graph, base_url, &site.config),
                    "rss",
                ));
            }
            if requested.contains(&"sitemap.xml") {
                artifacts.push((
                    "sitemap.xml",
                    sitemap::generate(&graph, base_url),
                    "sitemap",
                ));
            }
            if requested.contains(&"robots.txt") {
                let allow_paths = site
                    .config
                    .pointer("/discovery/machine_readable_paths")
                    .and_then(serde_json::Value::as_array)
                    .map(|paths| {
                        paths
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_else(|| {
                        vec!["page_references.json".into(), "static/tabsdata.json".into()]
                    });
                artifacts.push((
                    "robots.txt",
                    robots::generate(base_url, &allow_paths),
                    "robots",
                ));
            }
            for (path, contents, kind) in artifacts {
                let record = output::write_artifact(
                    &request.output_root,
                    std::path::Path::new(path),
                    contents.as_bytes(),
                    None,
                    kind,
                )?;
                if !matches!(record.state, crate::manifest::ArtifactState::Unchanged) {
                    manifest.changed.push(path.into());
                }
                manifest.generated.push(record);
            }
        }
    }
    if let Some(state_path) = request.state_path() {
        let state = incremental::state_from_build(
            &graph,
            &manifest,
            previous.as_ref(),
            global_fingerprint,
            static_files_for_state,
        );
        output::write_json(&state_path, &state)?;
    }
    image_cache
        .save(&request.image_cache_path())
        .map_err(|error| format!("image cache: {error}"))?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::{RenderEnvironment, RenderMode};
    use std::fs;

    #[test]
    fn selected_markdown_render_needs_no_css_tooling() {
        let root =
            std::env::temp_dir().join(format!("jnja-markdown-render-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("templates/layouts")).unwrap();
        fs::write(root.join("site_config.json"), r#"{"schema_version":2}"#).unwrap();
        fs::write(
            root.join("templates/page.md"),
            "---\ntitle: backend test\n---\n# Rendered Markdown\n",
        )
        .unwrap();
        fs::write(
            root.join("templates/layouts/markdown_page.html"),
            "<main>{{ content | safe }}</main>",
        )
        .unwrap();

        let output = root.join("output");
        let request = RenderRequest {
            schema_version: 1,
            site_root: root.clone(),
            config: Some(root.join("site_config.json")),
            output_root: output.clone(),
            mode: RenderMode::Selected,
            templates: vec!["page.md".into()],
            layout: Some("layouts/markdown_page.html".into()),
            context: None,
            environment: RenderEnvironment::Production,
            manifest: None,
            state: None,
            cache_dir: Some(root.join("cache")),
            prune: false,
            verify_static: false,
            render_only: true,
        };

        render(&request).unwrap();
        let html = fs::read_to_string(output.join("page/index.html")).unwrap();
        assert!(html.contains("Rendered Markdown"));
        assert!(!html.contains("tailwind"));
        let _ = fs::remove_dir_all(root);
    }
}
