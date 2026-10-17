use crate::{
    compatibility::markdown as compat, context::ContextMap, frontmatter::Document,
    template::TemplateEngine,
};
use comrak::markdown_to_html;
use minijinja::value::Value;

pub fn render_document(
    document: &Document,
    engine: &TemplateEngine,
    context: &mut ContextMap,
) -> Result<String, Box<dyn std::error::Error>> {
    let template_context = crate::context::as_template_value(context);
    render_document_with_value(document, engine, context, template_context)
}

pub fn render_document_with_value(
    document: &Document,
    engine: &TemplateEngine,
    context: &ContextMap,
    template_context: Value,
) -> Result<String, Box<dyn std::error::Error>> {
    let depth = context
        .get("depth")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let protected = compat::preprocess(&document.body, depth);
    let rendered =
        engine.render_inline_value("frontmatter-body.md", &protected, template_context)?;
    let rendered = compat::restore_protected(&rendered);
    let html = markdown_to_html(&rendered, &compat::options());
    Ok(compat::postprocess(html, &rendered))
}
