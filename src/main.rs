mod cli;

use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::Cli::parse(std::env::args().skip(1)) {
        Ok(cli::ParseOutcome::Run(cli)) => {
            cli.run();
            ExitCode::SUCCESS
        }
        Ok(cli::ParseOutcome::PrintHelp(text)) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            eprintln!("Run `codex-history --help` for usage.");
            ExitCode::from(2)
        }
    }
}
