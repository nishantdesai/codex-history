use std::process::ExitCode;

fn main() -> ExitCode {
    match codex_history::cli::Cli::parse(std::env::args().skip(1)) {
        Ok(codex_history::cli::ParseOutcome::Run(cli)) => match cli.run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!(
                    "error: {}",
                    codex_history::redact::redact_error_text(&message)
                );
                ExitCode::from(1)
            }
        },
        Ok(codex_history::cli::ParseOutcome::PrintHelp(text)) => {
            println!("{text}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!(
                "error: {}",
                codex_history::redact::redact_error_text(&message)
            );
            eprintln!("Run `codex-history --help` for usage.");
            ExitCode::from(2)
        }
    }
}
