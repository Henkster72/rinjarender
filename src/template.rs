use crate::{compatibility::jinja, context::ContextMap};
use minijinja::{path_loader, value::Value, AutoEscape, Environment};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub struct TemplateEngine {
    env: Environment<'static>,
}

impl TemplateEngine {
    pub fn new(template_root: &Path, depth: i64) -> Self {
        let mut env = Environment::new();
        env.set_loader(path_loader(template_root));
        env.set_auto_escape_callback(|_| AutoEscape::None);
        jinja::configure(&mut env);
        add_preprocessed_templates(&mut env, template_root, template_root, depth);
        Self { env }
    }
    pub fn new_site(template_root: &Path) -> Self {
        Self::new(template_root, 0)
    }
    pub fn render_named(
        &self,
        name: &str,
        context: &ContextMap,
    ) -> Result<String, minijinja::Error> {
        self.render_named_value(name, crate::context::as_template_value(context))
    }
    pub fn render_named_value(
        &self,
        name: &str,
        context: Value,
    ) -> Result<String, minijinja::Error> {
        self.env.get_template(name)?.render(context)
    }
    pub fn render_inline(
        &self,
        name: &str,
        source: &str,
        context: &ContextMap,
    ) -> Result<String, minijinja::Error> {
        self.render_inline_value(name, source, crate::context::as_template_value(context))
    }
    pub fn render_inline_value(
        &self,
        name: &str,
        source: &str,
        context: Value,
    ) -> Result<String, minijinja::Error> {
        self.env.render_named_str(name, source, context)
    }
}

fn add_preprocessed_templates(
    env: &mut Environment<'static>,
    root: &Path,
    directory: &Path,
    depth: i64,
) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            add_preprocessed_templates(env, root, &path, depth);
            continue;
        }
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if !matches!(extension, "html" | "htm") {
            continue;
        }
        let Ok(source) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let name = PathBuf::from(relative).to_string_lossy().replace('\\', "/");
        let _ = depth;
        let source = crate::compatibility::markdown::rewrite_asset_tokens_runtime(&source);
        let source = jinja::prepare_template(&source, root);
        let _ = env.add_template_owned(name, source);
    }
}

#[cfg(test)]
mod tests {
    use super::TemplateEngine;
    use crate::context::ContextMap;
    use serde_json::json;
    use std::fs;

    #[test]
    fn included_namespace_navigation_uses_page_route() {
        let root = std::env::temp_dir().join(format!("jnja-template-route-{}", std::process::id()));
        let macros = root.join("macros");
        fs::create_dir_all(&macros).unwrap();
        fs::write(
            macros.join("macro_header.html"),
            "{% set meet_brooklin = namespace(html='meet-brooklin.html' | url(depth)) %}<a href=\"{{ meet_brooklin.html }}\">Meet Brooklin</a>",
        )
        .unwrap();
        fs::write(
            root.join("base.html"),
            "{% include \"macros/macro_header.html\" %}",
        )
        .unwrap();
        let engine = TemplateEngine::new_site(&root);
        let mut context = ContextMap::new();
        context.insert("depth".into(), json!(0));
        let rendered = engine.render_named("base.html", &context).unwrap();
        assert!(rendered.contains("href=\"meet-brooklin/\""), "{rendered}");
        let _ = fs::remove_dir_all(root);
    }
}
