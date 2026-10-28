use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Parser, ValueEnum};
use codeuml_analysis::{analyze_project, AnalysisError, ProjectPath, ProjectPathError};
use codeuml_core::{Codebase, Diagnostic};
use codeuml_diagram::{
    project as project_diagram, render_projection_svg, DiagramScope, ProjectionError, SvgOptions,
};
use codeuml_tui::TuiError;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum FormatArg {
    /// Print the non-interactive analysis/projection summary.
    Summary,
    /// Render a standalone SVG diagram.
    Svg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum ScopeArg {
    Project,
    Module,
    Neighborhood,
}

/// Command-line interface for CodeUML.
#[derive(Debug, Parser)]
#[command(
    name = "codeuml",
    version,
    about = "Analyze source structure and explore it as UML-style diagrams"
)]
pub struct Cli {
    /// Project directory to analyze (defaults to the current directory).
    #[arg(value_name = "PROJECT_PATH")]
    pub project: Option<PathBuf>,

    /// Output format. Without this flag, terminals open the browser and
    /// non-interactive runs print a summary.
    #[arg(long, value_enum, value_name = "FORMAT")]
    pub format: Option<FormatArg>,

    /// Write SVG to a file instead of stdout. Existing files are not overwritten
    /// unless --force is supplied. Use "-" for stdout.
    #[arg(long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Diagram scope for SVG export.
    #[arg(long, value_enum, value_name = "SCOPE")]
    pub scope: Option<ScopeArg>,

    /// Module qualified name (or unique short name) for module scope.
    #[arg(long, value_name = "MODULE")]
    pub module: Option<String>,

    /// Type qualified name (or unique short name) for neighborhood scope.
    #[arg(long = "type", value_name = "TYPE")]
    pub type_name: Option<String>,

    /// Overwrite an existing SVG output file.
    #[arg(long)]
    pub force: bool,
}

impl Cli {
    /// SVG mode is explicit, or implied by file/scope selector options.
    pub fn svg_requested(&self) -> bool {
        match self.format {
            Some(FormatArg::Svg) => true,
            Some(FormatArg::Summary) => false,
            None => {
                self.output.is_some()
                    || self.scope.is_some()
                    || self.module.is_some()
                    || self.type_name.is_some()
                    || self.force
            }
        }
    }

    pub fn summary_requested(&self) -> bool {
        self.format == Some(FormatArg::Summary)
    }
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error(transparent)]
    ProjectPath(#[from] ProjectPathError),
    #[error(transparent)]
    Analysis(#[from] AnalysisError),
    #[error(transparent)]
    Projection(#[from] ProjectionError),
    #[error(transparent)]
    Tui(#[from] TuiError),
    #[error("{0}")]
    Usage(String),
    #[error("output path already exists: `{}` (use --force to overwrite)", .0.display())]
    OutputExists(PathBuf),
    #[error("could not write output: {0}")]
    Output(#[from] std::io::Error),
}

pub struct LoadedProject {
    pub path: ProjectPath,
    pub codebase: Codebase,
}

/// Resolve and analyze the requested project once for either UI mode.
pub fn load_project(cli: &Cli) -> Result<LoadedProject, CliError> {
    let path = ProjectPath::resolve(cli.project.as_deref())?;
    let codebase = analyze_project(&path)?;
    Ok(LoadedProject { path, codebase })
}

/// Non-interactive summary used for pipes, tests, and scripted environments.
pub fn run(cli: Cli, output: &mut impl Write) -> Result<(), CliError> {
    validate_summary_options(&cli)?;
    let loaded = load_project(&cli)?;
    let diagram = project_diagram(&loaded.codebase, &DiagramScope::Project)?;
    let metadata = loaded.codebase.analysis();

    writeln!(output, "CodeUML {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(output, "Project root: {}", loaded.path.as_path().display())?;
    writeln!(output, "Source files: {}", loaded.codebase.files().len())?;
    writeln!(output, "Types: {}", loaded.codebase.types().len())?;
    writeln!(
        output,
        "Relationships: {}",
        loaded.codebase.relationships().len()
    )?;
    writeln!(output, "Diagram nodes: {}", diagram.nodes.len())?;
    writeln!(output, "Diagram edges: {}", diagram.edges.len())?;
    writeln!(
        output,
        "Diagnostics: {}",
        loaded.codebase.diagnostics().len()
    )?;
    writeln!(
        output,
        "Analysis: {} ({} analyzed, {} failed{})",
        metadata.mode,
        metadata.files_analyzed,
        metadata.files_failed,
        if metadata.partial { ", partial" } else { "" }
    )?;

    Ok(())
}

/// Render the selected projection to SVG, writing diagnostics notices to a
/// separate stream so stdout remains a clean SVG document.
pub fn run_svg(
    cli: Cli,
    output: &mut impl Write,
    notices: &mut impl Write,
) -> Result<(), CliError> {
    validate_export_options(&cli)?;
    let loaded = load_project(&cli)?;
    let (scope, scope_label) = resolve_scope(&loaded.codebase, &cli)?;
    let projection = project_diagram(&loaded.codebase, &scope)?;
    let options = SvgOptions {
        title: format!("CodeUML: {}", loaded.codebase.name()),
        subtitle: Some(scope_label),
        ..SvgOptions::default()
    };
    let svg = render_projection_svg(&projection, &options);

    match cli.output.as_deref() {
        Some(path) if path != Path::new("-") => {
            write_svg_file(path, cli.force, svg.as_bytes())?;
            writeln!(notices, "codeuml: wrote {}", path.display())?;
        }
        _ => output.write_all(svg.as_bytes())?,
    }

    write_diagnostics(notices, loaded.codebase.diagnostics())?;
    Ok(())
}

/// Launch the interactive terminal browser. Analysis happens once before the
/// terminal is entered.
pub fn run_interactive(cli: Cli) -> Result<(), CliError> {
    let loaded = load_project(&cli)?;
    codeuml_tui::run_browser(&loaded.codebase)?;
    Ok(())
}

fn validate_summary_options(cli: &Cli) -> Result<(), CliError> {
    if cli.output.is_some()
        || cli.scope.is_some()
        || cli.module.is_some()
        || cli.type_name.is_some()
    {
        return Err(CliError::Usage(
            "export/scope options require SVG mode (use --format svg)".to_owned(),
        ));
    }
    if cli.force {
        return Err(CliError::Usage(
            "--force is only valid with a file SVG export".to_owned(),
        ));
    }
    Ok(())
}

fn validate_export_options(cli: &Cli) -> Result<(), CliError> {
    if cli.force {
        match cli.output.as_deref() {
            None => {
                return Err(CliError::Usage(
                    "--force requires --output with a file path".to_owned(),
                ));
            }
            Some(path) if path == Path::new("-") => {
                return Err(CliError::Usage(
                    "--force cannot be used when writing SVG to stdout".to_owned(),
                ));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

fn resolve_scope(codebase: &Codebase, cli: &Cli) -> Result<(DiagramScope, String), CliError> {
    let scope = match (cli.scope, cli.module.as_deref(), cli.type_name.as_deref()) {
        (Some(ScopeArg::Project), Some(_), _) | (Some(ScopeArg::Project), _, Some(_)) => {
            return Err(CliError::Usage(
                "--module/--type cannot be used with project scope".to_owned(),
            ));
        }
        (Some(ScopeArg::Module), _, Some(_)) => {
            return Err(CliError::Usage(
                "--type cannot be used with module scope".to_owned(),
            ));
        }
        (Some(ScopeArg::Module), None, _) => {
            return Err(CliError::Usage(
                "--scope module requires --module".to_owned(),
            ));
        }
        (Some(ScopeArg::Neighborhood), Some(_), _) => {
            return Err(CliError::Usage(
                "--module cannot be used with neighborhood scope".to_owned(),
            ));
        }
        (Some(ScopeArg::Neighborhood), _, None) => {
            return Err(CliError::Usage(
                "--scope neighborhood requires --type".to_owned(),
            ));
        }
        (None, Some(_), Some(_)) => {
            return Err(CliError::Usage(
                "use only one of --module or --type, or specify --scope".to_owned(),
            ));
        }
        (Some(scope), _, _) => scope,
        (None, Some(_), None) => ScopeArg::Module,
        (None, None, Some(_)) => ScopeArg::Neighborhood,
        (None, None, None) => ScopeArg::Project,
    };

    match scope {
        ScopeArg::Project => Ok((
            DiagramScope::Project,
            format!(
                "Scope: Project | {} types | {} relationships",
                codebase.types().len(),
                codebase.relationships().len()
            ),
        )),
        ScopeArg::Module => {
            let module = resolve_module(codebase, cli.module.as_deref().expect("validated"))?;
            Ok((
                DiagramScope::Module {
                    module: module.id.clone(),
                },
                format!("Scope: Module: {}", module.qualified_name),
            ))
        }
        ScopeArg::Neighborhood => {
            let declaration = resolve_type(codebase, cli.type_name.as_deref().expect("validated"))?;
            Ok((
                DiagramScope::TypeNeighborhood {
                    type_id: declaration.id.clone(),
                    direction: codeuml_diagram::NeighborhoodDirection::Both,
                },
                format!(
                    "Scope: Neighborhood: {} (bidirectional one-hop)",
                    declaration.qualified_name
                ),
            ))
        }
    }
}

fn resolve_module<'a>(
    codebase: &'a Codebase,
    selector: &str,
) -> Result<&'a codeuml_core::Module, CliError> {
    let exact: Vec<_> = codebase
        .modules()
        .iter()
        .filter(|module| module.qualified_name == selector)
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }
    let short: Vec<_> = codebase
        .modules()
        .iter()
        .filter(|module| module.name == selector)
        .collect();
    match short.as_slice() {
        [module] => Ok(*module),
        [] => Err(CliError::Usage(format!(
            "module `{selector}` not found; use a module qualified name from the summary or TUI"
        ))),
        _ => Err(CliError::Usage(format!(
            "module name `{selector}` is ambiguous; matches: {}",
            short
                .iter()
                .map(|module| module.qualified_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn resolve_type<'a>(
    codebase: &'a Codebase,
    selector: &str,
) -> Result<&'a codeuml_core::TypeDecl, CliError> {
    let exact: Vec<_> = codebase
        .types()
        .iter()
        .filter(|declaration| declaration.qualified_name == selector)
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }
    let short: Vec<_> = codebase
        .types()
        .iter()
        .filter(|declaration| declaration.name == selector)
        .collect();
    match short.as_slice() {
        [declaration] => Ok(*declaration),
        [] => Err(CliError::Usage(format!(
            "type `{selector}` not found; use a type qualified name from the browser"
        ))),
        _ => Err(CliError::Usage(format!(
            "type name `{selector}` is ambiguous; matches: {}",
            short
                .iter()
                .map(|declaration| declaration.qualified_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn write_svg_file(path: &Path, force: bool, bytes: &[u8]) -> Result<(), CliError> {
    let mut options = OpenOptions::new();
    options.write(true);
    if force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let mut file = options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            CliError::OutputExists(path.to_path_buf())
        } else {
            CliError::Output(error)
        }
    })?;
    file.write_all(bytes)?;
    file.flush()?;
    Ok(())
}

fn write_diagnostics(notices: &mut impl Write, diagnostics: &[Diagnostic]) -> Result<(), CliError> {
    if diagnostics.is_empty() {
        return Ok(());
    }
    writeln!(
        notices,
        "codeuml: {} analysis diagnostic(s); SVG reflects partial analysis.",
        diagnostics.len()
    )?;
    for diagnostic in diagnostics {
        let location = diagnostic
            .location
            .as_ref()
            .map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file.display(),
                    location.line,
                    location.column
                )
            })
            .unwrap_or_else(|| "<no location>".to_owned());
        let code = diagnostic
            .code
            .as_ref()
            .map(|code| format!(" [{code}]"))
            .unwrap_or_default();
        writeln!(
            notices,
            "codeuml: {} {location} {}{code}",
            diagnostic.severity, diagnostic.message
        )?;
    }
    Ok(())
}
