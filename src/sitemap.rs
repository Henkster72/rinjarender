use crate::{graph::SiteGraph, url::join_base_path};
pub fn generate(graph: &SiteGraph, base_url: &str) -> String {
    let mut out=String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">");
    for page in graph.logical_pages() {
        let url = join_base_path(base_url, &page.route);
        out.push_str(&format!(
            "<url><loc>{}</loc><priority>0.7</priority><changefreq>monthly</changefreq></url>",
            url
        ));
    }
    out.push_str("\n</urlset>\n");
    out
}
