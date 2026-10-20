# OpenSite Render

**A native Rust renderer for Jinja templates, Markdown and static sites.**

OpenSite Render uses the `opensite-render` CLI to render real Jinja-compatible
templates—not only simple variable substitution—and turns site source into
deterministic static files:

```text
templates + structured content + Markdown
                  ↓
         opensite-render
                  ↓
             static HTML/assets
```

It is designed for projects that want static HTML without requiring Node.js,
npm, a database, or a CMS runtime during rendering.

## Who is this for?

OpenSite Render is intended for projects that already use, or want to use:

- Jinja templates;
- Markdown or structured content files;
- static websites;
- CMS/editor frontends that publish HTML;
- Python or PHP render pipelines that should share one native renderer.

## Why this exists

Static-site workflows often grow around several overlapping renderers, large
JavaScript dependency trees, runtime CMS code, and separate local/server
implementations. This project keeps the rendering boundary deliberately small:

- one Rust render core;
- a portable executable;
- deterministic files as output;
- incremental rebuilds when useful;
- a thin orchestration layer around the renderer;
- optional post-processing instead of hidden renderer dependencies.

A traditional workflow often looks like this:

```text
Python/PHP + Jinja environment
        → load data → render templates → write static HTML
```

`opensite-render` moves the rendering part into one native executable. Python, PHP,
or another backend can still prepare data and orchestrate the process; they do
not need to be the production rendering runtime.

The goal is not to use Rust for its own sake. The goal is one renderer that can
be tested locally and invoked by another system without reproducing the entire
development environment on the server.

## What it replaces

The renderer can replace the rendering part of workflows based on:

- Python/Jinja render scripts;
- PHP template rendering;
- Node-based static rendering pipelines;
- ad-hoc Markdown-to-HTML scripts;
- duplicated local and server rendering logic.

It is not necessary to replace every surrounding tool. Existing CSS tooling,
content editors, upload commands, or deployment systems can remain outside the
renderer while the Rust core becomes the single HTML renderer.

## What it is not

OpenSite Render is not:

- a web server or CMS;
- a database or live page runtime;
- a JavaScript framework or browser bundler;
- a Tailwind implementation;
- a template editor;
- a deployment or upload system.

It generates static output. CSS optimization and deployment are separate
responsibilities.

The project is named **OpenSite Render** and the primary executable is
`opensite-render`. The existing `rinjarender` and `jnja-build` executables
remain available as backward-compatible aliases for local and integration
workflows.

## Jinja template support

OpenSite Render is intended for real Jinja template projects, not just
`{{ variable }}` substitution. Templates use MiniJinja plus the renderer's
compatibility layer.

The currently verified behavior is:

| Feature | Support |
|---|---|
| Variables and nested data | Yes |
| `if` / `elif` / `else` | Yes |
| `for` loops | Yes |
| `{% include %}` | Yes, including nested paths below the template root |
| `{% extends %}` and `{% block %}` | Yes |
| `{% macro %}` | Yes |
| `{% import %}` | Yes |
| `{% from ... import ... %}` | Yes |
| Built-in filters such as `upper` and `default` | Yes, where provided by MiniJinja |
| Project filters | `static`, `url`, `replace`, `split`, `startswith`, `endswith`, and `striptags` |
| Project functions | Compatibility `attribute` function |
| Undefined variables | Lenient by default; missing values render empty |
| Automatic HTML escaping | Disabled by default for compatibility |

This is a verified Jinja-compatible behavior surface, not a claim of 100%
compatibility with every Python Jinja extension. Python-only filters, arbitrary
Python functions, and application-specific globals are not automatically
available. Add them explicitly to the renderer or prepare their values in the
request context.

Automatic HTML escaping is disabled by default for compatibility. Applications
that render untrusted content must explicitly sanitize or escape that content
before treating generated HTML as trusted.

The fixture in `examples/jinja-features/` exercises inheritance, nested
includes, conditions, loops, macros, both import forms, structured data, and
filters:

```jinja
{% extends "base.html" %}
{% import "macros/ui.html" as ui %}

{% block content %}
  {% if featured %}
    {% for item in items %}<li>{{ item.name }}</li>{% endfor %}
  {% endif %}
  {{ ui.button("Read more", "/about/") }}
{% endblock %}
```

The same macro can be imported directly:

```jinja
{% from "macros/ui.html" import button %}
{{ button("Contact", "/contact/") }}
```

## Architecture

```text
site_config.json
templates
Markdown/content
structured data
       │
       ▼
   Rust render core
       │
       ├── full build
       ├── incremental build
       └── selected/content render
       │
       ▼
   static HTML/assets
       │
       └── optional post-processing
             ├── Tailwind purge
             ├── minification
             └── deployment
```

The same render core can be called by a site build, an editor/backend update,
or a server-side publication workflow. A selected Markdown/content update does
not require CSS tooling.

## Design principles

- **Deterministic:** identical source and configuration should produce stable
  output.
- **Static-first:** rendering produces files rather than runtime HTML.
- **Renderer independence:** the Rust renderer does not require Node, npm,
  Tailwind, or a database.
- **Incremental when possible:** relevant output is rebuilt from tracked source
  changes instead of rendering every page on every run.
- **One render core:** full, incremental, and selected rendering share the same
  rendering implementation.
- **Optional post-processing:** CSS optimization, uploads, and deployment stay
  outside the render core.

## Site layout

A site normally looks like this:

```text
my-site/
├── site_config.json
├── templates/
├── static/                 # optional assets
└── output/                 # generated; normally not source-controlled
```

The renderer discovers supported page sources below the site root. Exact
template and content conventions are project-specific; `site_config.json`
selects the portable site configuration and output path.

## Minimal example

This repository contains runnable examples in `examples/minimal/` and
`examples/jinja-features/`:

```text
examples/minimal/
├── site_config.json
└── templates/index.html
```

`site_config.json` contains the supported minimal schema marker:

```json
{
  "schema_version": 2
}
```

Render it with:

```bash
opensite-render examples/minimal --full
```

The result is written to:

```text
examples/minimal/output/index.html
```

The feature fixture is rendered as a normal full site build; it is not a
documentation-only mockup. It includes a Markdown page and the small
`layouts/markdown_page.html` layout used to render it.

## Quick start

Create a small site and preview it locally:

```bash
opensite-render init my-site
cd my-site
opensite-render serve
```

`serve` performs an initial incremental render, serves the generated `output/`
directory on `127.0.0.1:8000`, and rebuilds when source files change. It is a
development convenience, not a production web server. Use `--host` and
`--port` to change the listening address.

The generated `site_config.json` contains a `$schema` URL for editor
autocomplete. The formal schema is kept at
[`schemas/site-config-v2.schema.json`](schemas/site-config-v2.schema.json).

Build a site from any compatible checkout:

```bash
opensite-render /path/to/site --full
```

After the first build, use incremental mode during development:

```bash
opensite-render /path/to/site --incremental
```

When installed from source, the development binary is produced by:

```bash
cargo build --release
```

The resulting executable is under `target/release/`. A musl target can be used
when a portable static deployment artifact is required; that is a release and
deployment choice, not a requirement of local rendering.

The GitHub releases provide versioned binaries and SHA256 checksums. The v0.1.0
artifact retains the historical `jnja-build` filename; newer releases use the
public `opensite-render` name.

## CLI reference

The primary executable is `opensite-render`. `rinjarender` and `jnja-build`
remain compatibility aliases with the same implementation and behavior.

The Cargo package and library crate retain the historical internal name
`jnja-render` for source compatibility. This does not change the public CLI
name or the project name.

## Errors and cancellation

Command-line errors are written to stderr with a short context. Pressing
`Ctrl+C` cancels a build and exits with code `130`; `opensite-render serve` stops
cleanly. The renderer does not invoke CSS tooling as part of rendering.

The JSON build result is written only after a successful render, so backend
integrations can continue to use the structured output contract. Use the
development driver's `--debug` option when a full Python traceback or
subprocess diagnostic is needed.

| Argument | Purpose | Default |
|---|---|---|
| `SITE` | Site root | Current directory |
| `--site SITE` | Site root as an option | — |
| `--config PATH` | Configuration file | `SITE/site_config.json` |
| `--output DIR` | Generated output directory | Configured output or `SITE/output` |
| `--state-dir DIR` | Incremental state and cache directory | `~/.cache/jnja/local/<site-name>/` |
| `--full` | Full build | — |
| `--incremental` | Incremental build | Direct `SITE` invocation defaults here |
| `--render-only` | Render without static verification | Off |
| `--verify-static` | Enable static-output verification | Off |
| `--context PATH` | Add prepared JSON context for the render | None |
| `--help` | Show CLI help | — |

`--full` and `--incremental` are mutually exclusive. Relative paths are
resolved below the selected site root. The executable also retains explicit
automation flags such as `--site-root`, `--output-root`, `--state`, `--manifest`
and `--cache-dir` for backend integrations.

Configuration migration is a separate utility. Its public name is
`opensite-render-config-migrate`; `jnja-config-migrate` remains available as a
legacy alias:

```bash
opensite-render-config-migrate --site-root SITE --output site_config.json
```

## Full and incremental builds

Incremental state stores fingerprints, dependency information, and the latest
build result needed to plan subsequent work. The first incremental build may
render the whole site because no prior state exists. Later builds can report
only changed output and stale candidates.

The result contains information useful to CI and deployment tooling:

- `changed`: newly written or changed output artifacts;
- `stale_candidates`: artifacts the renderer owns but no longer produces;
- `generated`: the build's output artifact records;
- `warnings` and `errors`: diagnostics from the render.

The renderer does not delete arbitrary unknown files merely because they exist
in the output directory.

## Selected/content rendering

There is no separate active `jnja-render` executable required for normal use.
Backends can call the same Rust render core for a selected Markdown/content
update:

```text
CMS/editor/backend
        ↓
   Rust render core
        ↓
 updated static HTML
```

This path is intentionally independent of Tailwind, Node, npm, and CSS purge.
The current backend integration supplies the selected request and commits only
validated generated artifacts.

## CSS and Tailwind

`opensite-render` does not require Tailwind. CSS purge is an optional post-render
operation:

```text
Rust render → HTML → optional Tailwind/content scan → optimized CSS
```

When a higher-level build driver is used, a project may opt into purge with
configuration such as:

```json
{
  "tailwind": {
    "purge": true
  }
}
```

The renderer itself remains usable when the CSS tool is absent. This separation
also means that a content/editor update does not need to rebuild the site's CSS.

## `site-build` orchestration

`opensite-render` is the renderer CLI. `site-build` is a convenience driver around
it. Depending on the project, the driver can provide:

- interactive incremental/full mode selection;
- optional Tailwind purge and asset minification;
- site-specific preparation;
- validation and loading-budget checks;
- explicit upload choices.

These are orchestration concerns, not Rust renderer requirements. A generic
installation may use `opensite-render` directly and provide its own thin driver.

## Deployment model

A tested release artifact can be copied to a server and invoked there:

```text
build and test one binary
        ↓
copy the exact artifact to a server
        ↓
server invokes the same renderer
```

This avoids maintaining a second server-side rendering implementation and
reduces local runtime dependencies. The repository does not prescribe a
particular server, upload method, or process manager.

## Development

Run the normal Rust checks:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

The test suite covers full and incremental rendering, Markdown rendering,
configuration and stale-artifact behavior, and rendering without CSS tooling.

Focused fixture coverage also verifies variables, conditions, loops, includes,
inheritance, blocks, macros, imports, filters, and lenient undefined values.

## Traditional Jinja build compared with `opensite-render`

| | Traditional Python/Jinja build | `opensite-render` |
|---|---|---|
| Templates | Jinja | Verified Jinja-compatible subset |
| Output | Static HTML | Static HTML |
| Rendering runtime | Python plus packages | One Rust executable |
| Incremental state | Project-specific | Built in |
| Server renderer | Often separate setup | Same renderer artifact |
| Node/npm for rendering | Varies | Not required |
| CSS tooling | Separate | Separate |

## When should you use this?

Use `opensite-render` when:

- the final product is static HTML and assets;
- you want to retain a Jinja/Markdown-style source workflow;
- local and server rendering should use one native executable;
- incremental builds are useful;
- runtime database rendering is unnecessary.

It is probably not the right core when:

- the application requires React/Vue hydration as its primary runtime;
- pages are continuously database-driven at request time;
- a browser bundler is the central product requirement;
- you expect a complete CMS or visual editor from the renderer itself.

## Migration from an existing stack

Adoption can be incremental:

```text
existing templates/content
        ↓
validate renderer output
        ↓
use Rust for static rendering
        ↓
keep existing CSS/deployment tooling initially
        ↓
migrate orchestration separately when useful
```

The renderer does not require an immediate rewrite of every surrounding build
or deployment tool.
