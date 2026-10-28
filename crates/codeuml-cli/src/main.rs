use std::io::{self, IsTerminal};
use std::process::ExitCode;

use clap::Parser;
use codeuml_cli::{run, run_interactive, run_svg, Cli};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();

    let result = if cli.svg_requested() {
        let mut stdout = io::stdout().lock();
        let mut stderr = io::stderr().lock();
        run_svg(cli, &mut stdout, &mut stderr)
    } else if cli.summary_requested() || !interactive {
        let mut stdout = io::stdout().lock();
        run(cli, &mut stdout)
    } else {
        run_interactive(cli)
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
