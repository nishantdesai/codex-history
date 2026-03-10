#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Local,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalFlags {
    pub backend: Backend,
    pub json: bool,
    pub ndjson: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub no_color: bool,
}

impl Default for GlobalFlags {
    fn default() -> Self {
        Self {
            backend: Backend::Local,
            json: false,
            ndjson: false,
            quiet: false,
            verbose: false,
            no_color: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub global: GlobalFlags,
    pub command: Commands,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Commands {
    List,
    Show { thread_id: String },
    Search { query: String },
    Grep { pattern: String },
    Export { thread_id: String, format: String },
    Doctor,
    Index(IndexCommands),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexCommands {
    Build,
    Refresh,
    Doctor,
    Drop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseOutcome {
    Run(Cli),
    PrintHelp(String),
}

impl Cli {
    pub fn parse<I>(args: I) -> Result<ParseOutcome, String>
    where
        I: IntoIterator<Item = String>,
    {
        let args: Vec<String> = args.into_iter().collect();
        if args.is_empty() {
            return Ok(ParseOutcome::PrintHelp(top_level_help()));
        }

        let mut global = GlobalFlags::default();
        let mut command_start = 0;
        let mut wants_top_level_help = false;

        while let Some(arg) = args.get(command_start) {
            if arg == "--backend" {
                let value = args
                    .get(command_start + 1)
                    .ok_or_else(|| "missing value for --backend".to_string())?;
                global.backend = parse_backend(value)?;
                command_start += 2;
            } else if arg == "--json" {
                global.json = true;
                command_start += 1;
            } else if arg == "--ndjson" {
                global.ndjson = true;
                command_start += 1;
            } else if arg == "--quiet" {
                global.quiet = true;
                command_start += 1;
            } else if arg == "--verbose" {
                global.verbose = true;
                command_start += 1;
            } else if arg == "--no-color" {
                global.no_color = true;
                command_start += 1;
            } else if matches!(arg.as_str(), "-h" | "--help") {
                wants_top_level_help = true;
                command_start += 1;
            } else {
                break;
            }
        }

        validate_global_flags(&global)?;

        if wants_top_level_help {
            return Ok(ParseOutcome::PrintHelp(top_level_help()));
        }

        let remaining = &args[command_start..];
        let Some(command) = remaining.first() else {
            return Err("missing command".to_string());
        };

        if command.starts_with('-') {
            return Err(format!("unknown option: {command}"));
        }

        let parsed_command = match parse_command(remaining)? {
            ParsedCommandOutcome::Run(parsed_command) => parsed_command,
            ParsedCommandOutcome::PrintHelp(text) => return Ok(ParseOutcome::PrintHelp(text)),
        };

        if remaining.len() != parsed_command.consumed {
            return Err(unexpected_arguments(&remaining[parsed_command.consumed..]));
        }

        Ok(ParseOutcome::Run(Self {
            global,
            command: parsed_command.command,
        }))
    }

    pub fn run(&self) {
        match &self.command {
            Commands::List => println!("list: not implemented"),
            Commands::Show { .. } => println!("show: not implemented"),
            Commands::Search { .. } => println!("search: not implemented"),
            Commands::Grep { .. } => println!("grep: not implemented"),
            Commands::Export { .. } => println!("export: not implemented"),
            Commands::Doctor => println!("doctor: not implemented"),
            Commands::Index(index_command) => match index_command {
                IndexCommands::Build => println!("index build: not implemented"),
                IndexCommands::Refresh => println!("index refresh: not implemented"),
                IndexCommands::Doctor => println!("index doctor: not implemented"),
                IndexCommands::Drop => println!("index drop: not implemented"),
            },
        }
    }
}

fn parse_backend(value: &str) -> Result<Backend, String> {
    match value {
        "local" => Ok(Backend::Local),
        "auto" => Ok(Backend::Auto),
        other => Err(format!("invalid backend `{other}`; expected local|auto")),
    }
}

fn required_arg<'a>(args: &'a [String], index: usize, usage: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("missing required argument: {usage}"))
}

struct ParsedCommand {
    command: Commands,
    consumed: usize,
}

enum ParsedCommandOutcome {
    Run(ParsedCommand),
    PrintHelp(String),
}

fn validate_global_flags(global: &GlobalFlags) -> Result<(), String> {
    if global.json && global.ndjson {
        return Err("cannot combine --json and --ndjson".to_string());
    }

    if global.quiet && global.verbose {
        return Err("cannot combine --quiet and --verbose".to_string());
    }

    Ok(())
}

fn parse_command(args: &[String]) -> Result<ParsedCommandOutcome, String> {
    let command = match args[0].as_str() {
        "list" => ParsedCommandOutcome::Run(ParsedCommand {
            command: Commands::List,
            consumed: 1,
        }),
        "show" => ParsedCommandOutcome::Run(ParsedCommand {
            command: Commands::Show {
                thread_id: required_arg(args, 1, "show <thread-id>")?.to_string(),
            },
            consumed: 2,
        }),
        "search" => ParsedCommandOutcome::Run(ParsedCommand {
            command: Commands::Search {
                query: required_arg(args, 1, "search <query>")?.to_string(),
            },
            consumed: 2,
        }),
        "grep" => ParsedCommandOutcome::Run(ParsedCommand {
            command: Commands::Grep {
                pattern: required_arg(args, 1, "grep <pattern>")?.to_string(),
            },
            consumed: 2,
        }),
        "export" => ParsedCommandOutcome::Run(parse_export(args)?),
        "doctor" => ParsedCommandOutcome::Run(ParsedCommand {
            command: Commands::Doctor,
            consumed: 1,
        }),
        "index" => parse_index(args)?,
        other => return Err(format!("unknown command: {other}")),
    };

    Ok(command)
}

fn parse_export(args: &[String]) -> Result<ParsedCommand, String> {
    let thread_id = required_arg(args, 1, "export <thread-id>")?.to_string();
    let mut format = "json".to_string();
    let mut consumed = 2;

    if let Some(arg) = args.get(2) {
        if arg != "--format" {
            return Err(unexpected_arguments(&args[2..]));
        }

        format = required_arg(
            args,
            3,
            "export <thread-id> --format <json|markdown|prompt-pack>",
        )?
        .to_string();
        consumed = 4;
    }

    Ok(ParsedCommand {
        command: Commands::Export { thread_id, format },
        consumed,
    })
}

fn parse_index(args: &[String]) -> Result<ParsedCommandOutcome, String> {
    if args.len() == 1 {
        return Ok(ParsedCommandOutcome::PrintHelp(index_help()));
    }

    if matches!(args[1].as_str(), "-h" | "--help") {
        if args.len() == 2 {
            return Ok(ParsedCommandOutcome::PrintHelp(index_help()));
        }

        return Err(unexpected_arguments(&args[2..]));
    }

    let nested = match args[1].as_str() {
        "build" => IndexCommands::Build,
        "refresh" => IndexCommands::Refresh,
        "doctor" => IndexCommands::Doctor,
        "drop" => IndexCommands::Drop,
        other => return Err(format!("unknown index subcommand: {other}")),
    };

    Ok(ParsedCommandOutcome::Run(ParsedCommand {
        command: Commands::Index(nested),
        consumed: 2,
    }))
}

fn unexpected_arguments(args: &[String]) -> String {
    match args {
        [] => "unexpected argument".to_string(),
        [arg] => format!("unexpected argument: {arg}"),
        _ => format!("unexpected arguments: {}", args.join(" ")),
    }
}

fn top_level_help() -> String {
    "codex-history\nRead-only CLI for locally accessible Codex session history\n\nUSAGE:\n  codex-history [OPTIONS] <COMMAND>\n\nOPTIONS:\n  --backend <local|auto>\n  --json\n  --ndjson\n  --quiet\n  --verbose\n  --no-color\n  -h, --help\n\nCOMMANDS:\n  list\n  show <thread-id>\n  search <query>\n  grep <pattern>\n  export <thread-id> [--format <json|markdown|prompt-pack>]\n  doctor\n  index <build|refresh|doctor|drop>"
        .to_string()
}

fn index_help() -> String {
    "codex-history index\nManage opt-in local search index\n\nUSAGE:\n  codex-history index <COMMAND>\n\nCOMMANDS:\n  build\n  refresh\n  doctor\n  drop"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_global_backend_and_list() {
        let parsed = Cli::parse(vec!["--backend".into(), "auto".into(), "list".into()])
            .expect("parse success");
        let ParseOutcome::Run(cli) = parsed else {
            panic!("expected run");
        };
        assert_eq!(cli.global.backend, Backend::Auto);
        assert_eq!(cli.command, Commands::List);
    }

    #[test]
    fn prints_index_help() {
        let parsed = Cli::parse(vec!["index".into(), "--help".into()]).expect("parse success");
        assert!(matches!(parsed, ParseOutcome::PrintHelp(_)));
    }

    #[test]
    fn prints_top_level_help_after_global_flags() {
        let parsed = Cli::parse(vec!["--json".into(), "--help".into()]).expect("parse success");
        assert!(matches!(parsed, ParseOutcome::PrintHelp(_)));

        let parsed = Cli::parse(vec!["--backend".into(), "auto".into(), "--help".into()])
            .expect("parse success");
        assert!(matches!(parsed, ParseOutcome::PrintHelp(_)));
    }

    #[test]
    fn rejects_leftover_show_arguments() {
        let error = Cli::parse(vec!["show".into(), "thr_123".into(), "extra".into()])
            .expect_err("parse should fail");
        assert_eq!(error, "unexpected argument: extra");
    }

    #[test]
    fn rejects_leftover_index_arguments() {
        let error = Cli::parse(vec!["index".into(), "build".into(), "junk".into()])
            .expect_err("parse should fail");
        assert_eq!(error, "unexpected argument: junk");
    }

    #[test]
    fn rejects_missing_export_format_value() {
        let error = Cli::parse(vec!["export".into(), "thr_123".into(), "--format".into()])
            .expect_err("parse should fail");
        assert_eq!(
            error,
            "missing required argument: export <thread-id> --format <json|markdown|prompt-pack>"
        );
    }

    #[test]
    fn rejects_conflicting_global_flags() {
        let error = Cli::parse(vec!["--json".into(), "--ndjson".into(), "list".into()])
            .expect_err("parse should fail");
        assert_eq!(error, "cannot combine --json and --ndjson");

        let error = Cli::parse(vec!["--quiet".into(), "--verbose".into(), "list".into()])
            .expect_err("parse should fail");
        assert_eq!(error, "cannot combine --quiet and --verbose");
    }
}
