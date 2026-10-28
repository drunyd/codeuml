# CodeUML

CodeUML is a cross-platform command-line tool for analyzing Rust source
structure and exploring it through an interactive terminal browser or exporting
a standalone SVG diagram. It discovers Cargo projects, extracts declarations and
references without requiring a successful build, resolves references
conservatively, and projects a deterministic graph.

> **Status: Milestone 6.** Discovery, syntax analysis, conservative reference
> resolution, diagram projection, the Ratatui/Crossterm browser, an interactive
> terminal UML view, and SVG export are implemented. Python support, persistence,
> and compiler/semantic integration are not implemented.

## Workspace

| Crate | Responsibility |
| --- | --- |
| `codeuml-cli` | Executable, argument parsing, mode selection, composition |
| `codeuml-core` | Language-neutral model, stable IDs, relationships, diagnostics |
| `codeuml-analysis` | Cargo discovery, orchestration, reference resolution, validation |
| `codeuml-lang-rust` | `syn`-based Rust declarations, modules, and reference extraction |
| `codeuml-metrics` | CRAP report and `codeuml.toml` reader, score lookup and bands |
| `codeuml-diagram` | Deterministic projection, layered layout, and standalone SVG rendering |
| `codeuml-tui` | Ratatui/Crossterm browser, terminal UML layout/canvas, input, lifecycle |

Dependencies point one way: language and diagram crates consume the core
model; the TUI consumes the core model and diagram projection. The core crate
does not depend on any parser, renderer, or UI.

## Requirements

- Rust 1.85 or newer
- Cargo on `PATH` (discovery invokes `cargo metadata --no-deps`)
- An interactive terminal for the browser
- No network access or successful compilation is required

## Usage

```sh
cargo run -p codeuml-cli
cargo run -p codeuml-cli -- /path/to/project
cargo run -p codeuml-cli -- --help
cargo run -p codeuml-cli -- --version
```

Without a path, CodeUML analyzes the current working directory. The project
must contain a `Cargo.toml`.

- If both stdin and stdout are terminals, no explicit export options are present,
  and `--format` is omitted, CodeUML opens the interactive browser.
- If either stream is not a terminal (pipes, captured output, CI), CodeUML prints
  a non-interactive summary and exits without waiting for keyboard input.
- `--format svg`, `--output`, `--scope`, `--module`, `--type`, or `--force`
  selects SVG/export behavior and never launches the TUI.
- `--format summary` forces the non-interactive summary even in a terminal.
- Discovery/analysis failures return a nonzero exit status and an error message.
- Partial analysis results remain browsable and exportable; diagnostics are shown
  in the UI or written to stderr during SVG export.

Non-interactive summary example:

```text
CodeUML 0.1.0
Project root: /path/to/project
Source files: 43
Types: 105
Relationships: 462
Diagram nodes: 105
Diagram edges: 343
Diagnostics: 0
Analysis: syntax only (39 analyzed, 0 failed)
```

## SVG export

SVG export reuses the exact projection scope semantics and relationship data from
the terminal browser. It does not rerun analysis or independently resolve
references.

```sh
# Standalone SVG on stdout
codeuml --format svg > diagram.svg

# Write to a file (refuses to overwrite by default)
codeuml --format svg --output diagram.svg

# Explicitly overwrite
codeuml --format svg --output diagram.svg --force

# Module scope; selectors may be qualified names or unique short names
codeuml --format svg --scope module --module models --output modules.svg

# Selected-type neighborhood (bidirectional one-hop)
codeuml --format svg --scope neighborhood --type Customer --output customer.svg
```

Options:

| Option | Meaning |
| --- | --- |
| `--format summary\|svg` | Explicit output format |
| `--output <PATH>` | SVG destination; omitted or `-` means stdout |
| `--scope project\|module\|neighborhood` | Projection scope; default project |
| `--module <NAME>` | Module qualified name or unique short name |
| `--type <NAME>` | Type qualified name or unique short name |
| `--force` | Overwrite an existing SVG file |

Behavior:

- `--output` or any scope selector implies SVG mode when `--format` is omitted.
- Stdout in SVG mode contains only the XML document. Diagnostics and file-write
  notices go to stderr.
- Existing files are never overwritten without `--force`.
- Invalid or ambiguous module/type selectors return a clear nonzero error.
- Partial analysis still exports the usable model; every diagnostic is reported
  on stderr with severity, location, message, and code when available.
- `--force` requires a real file output path, not stdout.

### Initial SVG layout

- Types are placed in deterministic layers derived from an iterative topological
  level assignment; cycles and disconnected nodes receive stable fallback levels.
- Layers wrap into extra rows when the configured canvas width would be exceeded.
- Node widths/heights are based on rendered monospace text and are capped.
- Up to six members are shown per type by default; additional members are
  summarized as `+N more`.
- Polyline edges anchor to node borders with orthogonal routing and arrowheads.
- Boundary endpoints are placed below nodes as dashed capsules labeled
  `boundary: …` or `unresolved: …`; they are never drawn as project nodes.
- The SVG includes a title, scope subtitle, legend, accessible description,
  scalable `viewBox`, and deterministic element ordering.
- No external fonts, images, CSS, JavaScript, or network resources are used.

Relationship meaning is never conveyed by color alone: every edge carries its
relationship kind as text, arrowheads show source-to-target direction, and line
dashes distinguish method/trait/alias/boundary relationships. Trait
implementations are labeled `implements`, not inheritance.

## Terminal browser

The browser is a functional keyboard UI, not a graphical desktop application:

- **Header** — project, current scope, type/edge/diagnostic counts, analysis mode.
- **Types** — searchable deterministic type list with kind and qualified name.
- **Details** — qualified name, kind, visibility, source location, module,
  generics/bounds, alias target, fields, enum variants, methods, parameters, and
  return types.
- **Relationships** — outgoing (`OUT`) and incoming (`IN`) edges with kind,
  resolution state, target/source identity, and boundary/unresolved markers.
- **Diagram view** — a scrollable Unicode box-drawing UML canvas for the active
  project/module/neighborhood projection.
- **Footer** — selected status, search prompt, and contextual shortcuts.

### Keyboard shortcuts

| Key | Action |
| --- | --- |
| `Up`/`Down`, `j`/`k` | Move in the focused list |
| `PageUp`/`PageDown` | Move by a page |
| `Home`/`End` | First/last item |
| `Tab` | Switch between type list and relationships |
| `Enter` | Follow the selected relationship |
| `/` | Start incremental search |
| `Enter` (search) | Accept search and return to browsing |
| `Esc` (search) | Cancel search and restore the previous query/selection |
| `Esc` | Clear an active search or close the current view |
| `s` | Open scope dialog |
| `a` | Open the hierarchical module architecture browser |
| `d` | Open diagnostics |
| `Ctrl+U`/`Ctrl+D` | Scroll type details |
| `v` | Open the terminal UML diagram view |
| `o` | Show source for the selected type, module, or relationship evidence |
| `Backspace` | Go back after following a relationship |
| `?` | Show keyboard help overlay |
| `q` | Quit from the type browser |
| `Ctrl+C` | Quit from any view |

During search, ordinary characters—including `q`, `s`, `j`, and `?`—are treated
as text. `Ctrl+C` still quits.

### Module architecture browser

Press `a` from the type browser to navigate the analyzed module hierarchy in
three ranger-style panes:

- **Left** — the parent level, with the current module highlighted
  (`Project root` at the top).
- **Middle** — the current level; move with `j`/`k` or the arrow keys. Modules
  are shown as `name/`, types as `Name  [kind]`.
- **Right** — a live preview of the selected item: a module's children, or a
  type's full details.

`l`, `Right`, or `Enter` opens the selected module (or opens a type in the type
browser, scoped to its module). `h` or `Left` goes up a level, and stays put
at the project root. `Esc`/`b` goes up a level and, from the root, returns to
the type browser. `o` shows the selected item's source. On terminals narrower
than 90 columns the parent pane is hidden.

### Source view and back navigation

Press `o` to open a scrollable, line-numbered source view with the declaration
or evidence span highlighted. In the type list it shows the selected type; with
focus on the relationships panel it shows the selected relationship's first
evidence location; in the architecture browser and diagram view it shows the
selected module/type. Scroll with `Up`/`Down`, `PageUp`/`PageDown`, and
`Home`/`End`; `Esc`, `b`, or `o` returns to where you were.

Source paths come from analysis, so they are only read when they are
project-relative, resolve inside the project root (including through
symlinks), and are regular files under 2 MiB. Nothing is ever written.

Following a relationship records where you came from. `Backspace` returns to
the previous type and scope (up to 100 steps); the header shows the depth.
Type details also list each field, variant, and method with its source line.

### CRAP scores

If a [`crapper`](https://github.com/unclebob/crap4clj)-style report exists at
`.metrics/crap.edn`, the browser shows each module, type, and method's CRAP score
on the same line as its name, right-aligned and colour-coded:

| Colour | Default band |
| --- | --- |
| green | score <= 6 |
| yellow | score <= 12 |
| red | above 12 |

- **Methods** show their own score (the worst one when a name appears more than
  once on a type). **Types** show the worst score among their methods, and
  **modules** the worst among everything in them and their sub-modules, so one
  bad function stays visible from every level above it.
- Scores appear in the type list, all three architecture panes, and the type
  details (on the `Name:` line and each method). The header legend shows the
  active limits. The number is always printed, so colour is never the only cue.
- Limits and the report path are configured in `codeuml.toml` at the project
  root; both settings are optional:

  ```toml
  [crap]
  good_max = 6
  medium_max = 12
  report = ".metrics/crap.edn"
  ```

- A missing default report is silently ignored. A broken `codeuml.toml`,
  a missing configured report, or an unreadable report is reported as a warning
  in the diagnostics view (`d`) and CodeUML falls back to the defaults.
- Report namespaces use underscores (`codeuml_tui::app::App`) and are matched to
  package names with hyphens (`codeuml-tui`). Functions outside any shown type or
  module, such as free functions in a binary's `main.rs`, are not displayed.

### Terminal UML diagram view

Press `v` from the browser to open a Unicode diagram of the current projection.
The view reuses the same project/module/neighborhood scope and relationship data
as the browser and SVG export; it does not rerun analysis.

- Type nodes are drawn with box-drawing borders and include type name, kind,
  visibility, qualified name when relevant, and up to four compact members.
  Omitted members are marked with `+N more`.
- The focused type is highlighted. `Tab`/`BackTab` or `n`/`p` move focus through
  the diagram nodes and recenter when practical.
- Arrow keys or `h`/`j`/`k`/`l` pan the viewport. `PageUp`/`PageDown` pan a page.
  `c` recenters on the focused type.
- `Enter` returns to the browser with the focused type selected, opening its
  existing details and relationship panels.
- `Esc`, `b`, or `v` returns to the browser.
- `s` opens scope selection, `d` diagnostics, and `?` help. Closing those views
  returns to the diagram when they were opened from it.
- The viewport is clipped to the terminal body and recalculated on resize.
  Focused-node visibility is retained where practical.
- Boundary/unresolved endpoints are rendered as dashed labelled capsules below
  project nodes. Relationship direction uses arrowheads and relationship kind is
  written as a label when space permits; line colors provide an additional cue.
- Self references, multiple kinds between the same nodes, dense fan-out, and
  graphs larger than the viewport are supported. There is no zoom in this
  milestone.

Diagram keyboard shortcuts:

| Key | Action |
| --- | --- |
| `Up`/`Down`/`Left`/`Right`, `h`/`j`/`k`/`l` | Pan viewport |
| `PageUp`/`PageDown` | Pan by a page |
| `Tab`/`BackTab`, `n`/`p` | Focus next/previous type |
| `c` | Center on focused type |
| `Enter` | Open focused type in the browser/details view |
| `Esc`/`b`/`v` | Return to browser |
| `s` | Scope selection |
| `d` | Diagnostics |
| `?` | Help |
| `q` | Quit |
| `Ctrl+C` | Quit from any view |

### Search

- Incremental, case-insensitive substring search.
- Matches both short names and qualified names.
- Filtering only reuses the existing projection; source analysis does not rerun.
- Empty results display a clear message.
- A valid selection is preserved when possible; otherwise selection falls back
  to the first deterministic result.

### Scope selection

Press `s` to choose from:

- **Project** — all project types and relationships.
- **Module** — one known module. Types directly in the module are nodes;
  relationships touching the module remain visible, with resolved outside
  endpoints shown as boundary endpoints.
- **Neighborhood** — the selected type and one hop of direct bidirectional
  neighbors. Neighbor expansion is not recursive, so cycles stay bounded.

Switching scope rebuilds the projection with the existing `codeuml-diagram` API.
The selected type is retained if still visible; otherwise the first deterministic
valid item is selected. Failed/stale scope targets show feedback and preserve the
previous usable view.

### Relationship navigation

`Tab` focuses the relationship list. `Enter` follows:

- A resolved node target: selects that type.
- A resolved boundary target outside the current scope: opens it in project
  scope (clearing search if needed).
- An unresolved or external boundary target: shows a message and does not
  navigate; no fake declaration is created.

Trait implementation appears as the `Implements` relationship, not class
inheritance.

### Diagnostics

Press `d` to browse severity, message, source file/line/column, and diagnostic
code. `Up`/`Down`, `PageUp`/`PageDown`, and `Home`/`End` navigate. `Esc` returns
to the browser. Zero diagnostics and missing locations are handled explicitly.

### Terminal lifecycle and testability

- Raw mode and the alternate screen are entered only in interactive mode.
- An RAII guard restores raw mode and leaves the alternate screen on normal exit,
  early returns, rendering errors, and event-loop errors.
- The original error is preserved if cleanup also fails.
- State transitions, key mapping, and rendering are tested without a real
  terminal using ordinary unit tests and Ratatui's `TestBackend`.

## What is analyzed

- Single Cargo packages and multi-package workspaces via Cargo metadata.
- Package `src` module trees and Cargo target sources (bins, tests, examples,
  benches, build scripts).
- Structs, enums and variants, unions, traits, type aliases, inherent and trait
  `impl` blocks.
- Visibility, generics and bounds, methods, associated functions, parameters,
  and return types.
- Inline modules and conventional external modules (`foo.rs`, `foo/mod.rs`).
- Project-relative source locations and stable, module-qualified IDs.

Files are parsed independently with `syn`. A malformed file produces a
structured diagnostic while declarations from other files remain available.

Discovery excludes `target`, `.git`, and nested `vendor` directories, and does
not follow symbolic links. Runs do not execute project code or compile it.

## Relationship semantics

- `FieldType` — field or enum-variant field type reference.
- `MethodParameterType` — method parameter type reference.
- `MethodReturnType` — method return type reference.
- `Aliases` — type-alias target reference.
- `TraitBound` — generic/where/`impl Trait`/trait-object/supertrait bound.
- `Implements` — Rust trait implementation.
- `References` — nested type references inside trait-bound generic arguments.
- `Contains` and `Extends` exist in the model but are not emitted by Rust analysis.

Member-level references use the field or method as the source; projection maps
them to owning type nodes. Unresolved references remain in the model with
`Unresolved` status and retain evidence.

## Resolution

Resolution is syntax/project-context based, never compiler-accurate:

- Handles local module scope, `crate::`, `self::`, `super::`, named imports,
  aliases, glob imports, and workspace crate names.
- Resolves nested paths and generic arguments without creating declarations for
  primitives, generic parameters, containers, or external types.
- Requires a unique candidate. Ambiguous references remain unresolved and emit
  an `analysis::ambiguous-reference` diagnostic.
- Preserves unresolved local/unknown external references; known prelude and
  `std`/`core`/`alloc` heads are treated as external.
- Resolves workspace cross-crate references when a visible crate alias/path is
  unambiguous.
- Merges observations by `(source, target, kind)` while retaining evidence.
- Marks syntax-derived targets as `Inferred`, not semantic `Resolved`.

## Diagram projection scopes

- **Project** — every project type and relationship from a project declaration.
- **Module** — module-owned types plus all relationships touching the module;
  resolved outside endpoints become boundaries, not implicit nodes.
- **Type neighborhood** — selected type plus one hop in the chosen direction;
  no recursive traversal.

Projection merges member-level relationships into deterministic node-level
edges and retains kind, resolution, provenance, evidence, and source members.

## Checks

```sh
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
git diff --cached --check
```

## Known limitations

- Syntax analysis only; no rust-analyzer or compiler-level resolution.
- Imports/re-exports are approximated; `pub use` chains and module visibility
  are not modeled.
- Cross-crate resolution does not verify the Cargo dependency graph.
- Local-vs-glob precedence and other Rust name-resolution edge cases are
  conservative rather than exact.
- Associated types and qualified associated paths remain unresolved.
- `#[path]` modules are skipped; `cfg`/macro-generated code is not evaluated.
- Module files follow conventional Rust/Cargo layouts from discovery.
- Known external/prelude names are a heuristic.
- Start line/column positions are the primary tested source-location contract.
- Diagram nodes are type declarations; modules are scopes, not nodes.
- The browser and terminal diagram are keyboard-only (no mouse) and have no
  persistent state or configuration.
- The terminal diagram does not implement zoom, arbitrary placement, or
  crossing-minimized routing. Edge labels may be omitted when no collision-free
  space is available.
- Diagram node detail is intentionally compact; use the existing details panel
  for complete members.
- SVG layout uses a deterministic layered/grid fallback rather than a
  crossing-minimizing graph-layout engine; some edge crossings and long routes
  are expected.
- SVG nodes cap member display and truncate long text; use the TUI for complete
  member details.
- SVG rendering uses a monospace width estimate rather than embedded font
  metrics.
- No Python support, semantic enrichment, or compiler execution.

## Next milestone

Potential next work: cross-platform packaging/CI and optional semantic enrichment
behind a provider interface, or Python as a separate language crate. None of
these are implemented here.
