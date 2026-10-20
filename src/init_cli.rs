use std::{
    fs,
    path::{Path, PathBuf},
};

const CONFIG: &str = r#"{
  "$schema": "https://raw.githubusercontent.com/Henkster72/opensite-render/main/schemas/site-config-v2.schema.json",
  "schema_version": 2,
  "paths": {
    "templates": "templates",
    "static": "static",
    "output": "output"
  },
  "site": {
    "name": "OpenSite Render site",
    "language": "en"
  }
}
"#;

fn write(path: &Path, contents: &str) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(path, contents)?;
    Ok(())
}

pub fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("opensite-render init DIRECTORY");
        return Ok(());
    }
    let target = args
        .first()
        .map(PathBuf::from)
        .ok_or("missing target directory")?;
    if target.exists() && target.read_dir()?.next().is_some() {
        return Err(format!(
            "refusing to initialize non-empty directory: {}",
            target.display()
        )
        .into());
    }
    fs::create_dir_all(target.join("templates/layouts"))?;
    fs::create_dir_all(target.join("static"))?;
    fs::create_dir_all(target.join("content"))?;
    write(&target.join("site_config.json"), CONFIG)?;
    write(
        &target.join("templates/base.html"),
        "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <title>{% block title %}OpenSite Render site{% endblock %}</title>\n</head>\n<body>\n  <main>{% block content %}{% endblock %}</main>\n</body>\n</html>\n",
    )?;
    write(
        &target.join("templates/index.html"),
        "{% extends \"base.html\" %}\n{% block title %}Welcome{% endblock %}\n{% block content %}\n  <h1>{{ site.name }}</h1>\n  {% if site.language %}<p>Language: {{ site.language }}</p>{% endif %}\n{% endblock %}\n",
    )?;
    write(
        &target.join("templates/layouts/markdown_page.html"),
        "{% extends \"base.html\" %}\n{% block content %}{{ body }}{% endblock %}\n",
    )?;
    write(
        &target.join("content/index.md"),
        "# Content\n\nAdd Markdown pages under the template/content conventions used by your site.\n",
    )?;
    write(
        &target.join(".gitignore"),
        "output/\n.cache/\n.env\n.env.*\n",
    )?;
    println!("initialized {}", target.display());
    Ok(())
}
