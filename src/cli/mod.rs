use serde::Serialize;

use crate::backend::local::{GrepMatch, LocalBackend, LocalDoctorReport};
use crate::index::ingest::{
    build_local_index, load_manifest_snapshot, refresh_local_index, IndexBuildReport,
    IndexRefreshReport,
};
use crate::index::manifest::default_index_path;
use crate::index::query::{search_index, search_with_fresh_overlay, SearchResult};
use crate::index::schema::{doctor as doctor_index, IndexDoctorReport};
use crate::model::{Item, ThreadDetail, ThreadSummary};

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
    Show {
        thread_id: String,
        include_turns: bool,
    },
    Search {
        query: String,
        fresh: bool,
    },
    Grep {
        pattern: String,
        regex: bool,
    },
    Export {
        thread_id: String,
        format: String,
    },
    Doctor,
    Index(IndexCommands),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexCommands {
    Build,
    Refresh,
    Doctor,
    Drop { yes: bool },
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

    pub fn run(&self) -> Result<(), String> {
        let backend = self.local_backend();

        match &self.command {
            Commands::List => {
                let threads = backend.list_threads()?;
                render_collection(&self.global, &threads, render_thread_summary)
            }
            Commands::Show {
                thread_id,
                include_turns,
            } => {
                let detail = backend
                    .show_thread(thread_id, *include_turns)?
                    .ok_or_else(|| format!("thread not found: {thread_id}"))?;
                render_single(&self.global, &detail, |detail| {
                    render_thread_detail(detail, *include_turns)
                })
            }
            Commands::Search { .. } => {
                let Commands::Search { query, fresh } = &self.command else {
                    unreachable!("matched command variant");
                };
                let path = default_index_path();
                let results = if *fresh {
                    let manifest = load_manifest_snapshot(&path)?;
                    let details = backend.list_thread_details()?;
                    search_with_fresh_overlay(&path, query, 50, &details, &manifest)?
                } else {
                    search_index(&path, query, 50)?
                };
                render_collection(&self.global, &results, render_search_result)
            }
            Commands::Grep { pattern, regex } => {
                let matches = backend.grep(pattern, *regex)?;
                render_collection(&self.global, &matches, render_grep_match)
            }
            Commands::Export { .. } => {
                println!("export: not implemented");
                Ok(())
            }
            Commands::Doctor => {
                let report = backend.doctor()?;
                render_single(&self.global, &report, render_doctor_report)
            }
            Commands::Index(index_command) => match index_command {
                IndexCommands::Build => {
                    let report = build_local_index(&backend, &default_index_path())?;
                    render_single(&self.global, &report, render_index_build_report)
                }
                IndexCommands::Refresh => {
                    let report = refresh_local_index(&backend, &default_index_path())?;
                    render_single(&self.global, &report, render_index_refresh_report)
                }
                IndexCommands::Doctor => {
                    let report = doctor_index(&default_index_path())?;
                    render_single(&self.global, &report, render_index_doctor_report)
                }
                IndexCommands::Drop { .. } => Err("index drop is not implemented yet".into()),
            },
        }
    }

    fn local_backend(&self) -> LocalBackend {
        match self.global.backend {
            Backend::Local | Backend::Auto => LocalBackend::discover(),
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
        "list" => parse_list(args)?,
        "show" => parse_show(args)?,
        "search" => parse_search(args)?,
        "grep" => parse_grep(args)?,
        "export" => parse_export_command(args)?,
        "doctor" => parse_doctor(args)?,
        "index" => parse_index(args)?,
        other => return Err(format!("unknown command: {other}")),
    };

    Ok(command)
}

fn parse_list(args: &[String]) -> Result<ParsedCommandOutcome, String> {
    if args.len() == 2 && is_help_flag(&args[1]) {
        return Ok(ParsedCommandOutcome::PrintHelp(list_help()));
    }

    ensure_no_command_args(args)?;

    Ok(ParsedCommandOutcome::Run(ParsedCommand {
        command: Commands::List,
        consumed: args.len(),
    }))
}

fn parse_show(args: &[String]) -> Result<ParsedCommandOutcome, String> {
    if args.len() == 2 && is_help_flag(&args[1]) {
        return Ok(ParsedCommandOutcome::PrintHelp(show_help()));
    }

    let mut thread_id = None;
    let mut include_turns = false;

    for arg in &args[1..] {
        match arg.as_str() {
            "--include-turns" => include_turns = set_flag(include_turns, "--include-turns")?,
            arg if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ if thread_id.is_none() => thread_id = Some(arg.clone()),
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }

    Ok(ParsedCommandOutcome::Run(ParsedCommand {
        command: Commands::Show {
            thread_id: thread_id
                .ok_or_else(|| "missing required argument: show <thread-id>".to_string())?,
            include_turns,
        },
        consumed: args.len(),
    }))
}

fn parse_search(args: &[String]) -> Result<ParsedCommandOutcome, String> {
    if args.len() == 2 && is_help_flag(&args[1]) {
        return Ok(ParsedCommandOutcome::PrintHelp(search_help()));
    }

    let mut query = None;
    let mut fresh = false;
    let mut end_of_options = false;

    for (index, arg) in args[1..].iter().enumerate() {
        if end_of_options {
            match query.as_ref() {
                None => query = Some(arg.clone()),
                Some(_) => return Err(format!("unexpected argument: {arg}")),
            }
            continue;
        }

        match arg.as_str() {
            "--fresh" => fresh = set_flag(fresh, "--fresh")?,
            "--" => end_of_options = true,
            arg if arg.starts_with('-') && query.is_none() && index + 2 == args.len() => {
                query = Some(arg.to_string())
            }
            arg if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            arg if query.is_none() && !arg.starts_with('-') => query = Some(arg.to_string()),
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }

    Ok(ParsedCommandOutcome::Run(ParsedCommand {
        command: Commands::Search {
            query: query.ok_or_else(|| "missing required argument: search <query>".to_string())?,
            fresh,
        },
        consumed: args.len(),
    }))
}

fn parse_grep(args: &[String]) -> Result<ParsedCommandOutcome, String> {
    if args.len() == 2 && is_help_flag(&args[1]) {
        return Ok(ParsedCommandOutcome::PrintHelp(grep_help()));
    }

    let mut pattern = None;
    let mut regex = false;
    let mut end_of_options = false;

    for (index, arg) in args[1..].iter().enumerate() {
        if end_of_options {
            match pattern.as_ref() {
                None => pattern = Some(arg.clone()),
                Some(_) => return Err(format!("unexpected argument: {arg}")),
            }
            continue;
        }

        match arg.as_str() {
            "--regex" => regex = set_flag(regex, "--regex")?,
            "--" => end_of_options = true,
            arg if arg.starts_with('-') && pattern.is_none() && index + 2 == args.len() => {
                pattern = Some(arg.to_string())
            }
            arg if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            arg if pattern.is_none() && !arg.starts_with('-') => pattern = Some(arg.to_string()),
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }

    Ok(ParsedCommandOutcome::Run(ParsedCommand {
        command: Commands::Grep {
            pattern: pattern
                .ok_or_else(|| "missing required argument: grep <pattern>".to_string())?,
            regex,
        },
        consumed: args.len(),
    }))
}

fn parse_export(args: &[String]) -> Result<ParsedCommand, String> {
    let mut thread_id = None;
    let mut format = "json".to_string();
    let mut saw_format = false;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--format" => {
                if saw_format {
                    return Err("duplicate option: --format".to_string());
                }

                let value = required_arg(
                    args,
                    i + 1,
                    "export <thread-id> --format <json|markdown|prompt-pack>",
                )?;
                format = parse_export_format(value)?.to_string();
                saw_format = true;
                i += 2;
            }
            arg if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ if thread_id.is_none() => {
                thread_id = Some(args[i].clone());
                i += 1;
            }
            _ => return Err(format!("unexpected argument: {}", args[i])),
        }
    }

    Ok(ParsedCommand {
        command: Commands::Export {
            thread_id: thread_id
                .ok_or_else(|| "missing required argument: export <thread-id>".to_string())?,
            format,
        },
        consumed: args.len(),
    })
}

fn parse_doctor(args: &[String]) -> Result<ParsedCommandOutcome, String> {
    if args.len() == 2 && is_help_flag(&args[1]) {
        return Ok(ParsedCommandOutcome::PrintHelp(doctor_help()));
    }

    ensure_no_command_args(args)?;

    Ok(ParsedCommandOutcome::Run(ParsedCommand {
        command: Commands::Doctor,
        consumed: args.len(),
    }))
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

    if args.len() == 3 && is_help_flag(&args[2]) {
        return Ok(ParsedCommandOutcome::PrintHelp(index_subcommand_help(
            args[1].as_str(),
        )?));
    }

    let nested = match args[1].as_str() {
        "build" => {
            ensure_no_index_args(args, "build")?;
            IndexCommands::Build
        }
        "refresh" => {
            ensure_no_index_args(args, "refresh")?;
            IndexCommands::Refresh
        }
        "doctor" => {
            ensure_no_index_args(args, "doctor")?;
            IndexCommands::Doctor
        }
        "drop" => IndexCommands::Drop {
            yes: parse_index_drop(args)?,
        },
        other => return Err(format!("unknown index subcommand: {other}")),
    };

    Ok(ParsedCommandOutcome::Run(ParsedCommand {
        command: Commands::Index(nested),
        consumed: args.len(),
    }))
}

fn parse_index_drop(args: &[String]) -> Result<bool, String> {
    let mut yes = false;

    for arg in &args[2..] {
        match arg.as_str() {
            "--yes" => yes = set_flag(yes, "--yes")?,
            arg if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }

    Ok(yes)
}

fn parse_export_command(args: &[String]) -> Result<ParsedCommandOutcome, String> {
    if args.len() == 2 && is_help_flag(&args[1]) {
        return Ok(ParsedCommandOutcome::PrintHelp(export_help()));
    }

    Ok(ParsedCommandOutcome::Run(parse_export(args)?))
}

fn ensure_no_command_args(args: &[String]) -> Result<(), String> {
    match args.len() {
        1 => Ok(()),
        2 if args[1].starts_with('-') => Err(format!("unknown option: {}", args[1])),
        2 => Err(format!("unexpected argument: {}", args[1])),
        _ => Err(unexpected_arguments(&args[1..])),
    }
}

fn ensure_no_index_args(args: &[String], subcommand: &str) -> Result<(), String> {
    match args.len() {
        2 => Ok(()),
        3 if args[2].starts_with('-') => Err(format!("unknown option: {}", args[2])),
        3 => Err(format!("unexpected argument: {}", args[2])),
        _ => Err(unexpected_arguments(&args[2..])),
    }
    .map_err(|error| {
        if error == "unexpected argument: --help" {
            format!("unexpected argument after `index {subcommand}`: --help")
        } else {
            error
        }
    })
}

fn set_flag(current: bool, flag: &str) -> Result<bool, String> {
    if current {
        Err(format!("duplicate option: {flag}"))
    } else {
        Ok(true)
    }
}

fn parse_export_format(value: &str) -> Result<&str, String> {
    match value {
        "json" | "markdown" | "prompt-pack" => Ok(value),
        other => Err(format!(
            "invalid export format `{other}`; expected json|markdown|prompt-pack"
        )),
    }
}

fn is_help_flag(arg: &str) -> bool {
    matches!(arg, "-h" | "--help")
}

fn unexpected_arguments(args: &[String]) -> String {
    match args {
        [] => "unexpected argument".to_string(),
        [arg] => format!("unexpected argument: {arg}"),
        _ => format!("unexpected arguments: {}", args.join(" ")),
    }
}

fn top_level_help() -> String {
    "codex-history\nRead-only CLI for locally accessible Codex session history\n\nUSAGE:\n  codex-history [OPTIONS] <COMMAND>\n\nOPTIONS:\n  --backend <local|auto>\n  --json\n  --ndjson\n  --quiet\n  --verbose\n  --no-color\n  -h, --help\n\nCOMMANDS:\n  list\n  show <thread-id>\n  search <query>\n  grep <pattern>\n  export <thread-id>\n  doctor\n  index <build|refresh|doctor|drop>\n\nRun `codex-history <COMMAND> --help` for command usage."
        .to_string()
}

fn index_help() -> String {
    "codex-history index\nManage the opt-in local search index\n\nUSAGE:\n  codex-history [OPTIONS] index <COMMAND>\n\nCOMMANDS:\n  build\n  refresh\n  doctor\n  drop\n\nRun `codex-history index <COMMAND> --help` for command usage."
        .to_string()
}

fn list_help() -> String {
    "codex-history list\nList locally accessible Codex threads/sessions\n\nUSAGE:\n  codex-history [OPTIONS] list\n\nOPTIONS:\n  -h, --help"
        .to_string()
}

fn show_help() -> String {
    "codex-history show\nShow thread metadata and optionally full turns\n\nUSAGE:\n  codex-history [OPTIONS] show [--include-turns] <thread-id>\n\nOPTIONS:\n  --include-turns\n  -h, --help"
        .to_string()
}

fn search_help() -> String {
    "codex-history search\nSearch across history\n\nUSAGE:\n  codex-history [OPTIONS] search [--fresh] <query>\n\nOPTIONS:\n  --fresh\n  -h, --help"
        .to_string()
}

fn grep_help() -> String {
    "codex-history grep\nLiteral or regex transcript search without ranking\n\nUSAGE:\n  codex-history [OPTIONS] grep [--regex] <pattern>\n\nOPTIONS:\n  --regex\n  -h, --help"
        .to_string()
}

fn export_help() -> String {
    "codex-history export\nExport a thread\n\nUSAGE:\n  codex-history [OPTIONS] export <thread-id> [--format <json|markdown|prompt-pack>]\n\nOPTIONS:\n  --format <json|markdown|prompt-pack>\n  -h, --help"
        .to_string()
}

fn doctor_help() -> String {
    "codex-history doctor\nCheck Codex history roots and index paths\n\nUSAGE:\n  codex-history [OPTIONS] doctor\n\nOPTIONS:\n  -h, --help"
        .to_string()
}

fn index_subcommand_help(subcommand: &str) -> Result<String, String> {
    let help = match subcommand {
        "build" => {
            "codex-history index build\nBuild the local index from scratch\n\nUSAGE:\n  codex-history [OPTIONS] index build\n\nOPTIONS:\n  -h, --help"
        }
        "refresh" => {
            "codex-history index refresh\nRefresh changed or new threads in the local index\n\nUSAGE:\n  codex-history [OPTIONS] index refresh\n\nOPTIONS:\n  -h, --help"
        }
        "doctor" => {
            "codex-history index doctor\nCheck index integrity and staleness\n\nUSAGE:\n  codex-history [OPTIONS] index doctor\n\nOPTIONS:\n  -h, --help"
        }
        "drop" => {
            "codex-history index drop\nDrop the local index\n\nUSAGE:\n  codex-history [OPTIONS] index drop [--yes]\n\nOPTIONS:\n  --yes\n  -h, --help"
        }
        other => return Err(format!("unknown index subcommand: {other}")),
    };

    Ok(help.to_string())
}

fn render_collection<T, F>(
    global: &GlobalFlags,
    values: &[T],
    render_human: F,
) -> Result<(), String>
where
    T: Serialize,
    F: Fn(&T) -> String,
{
    if global.json {
        print_json(values)
    } else if global.ndjson {
        print_ndjson(values)
    } else {
        for value in values {
            println!("{}", render_human(value));
        }
        Ok(())
    }
}

fn render_single<T, F>(global: &GlobalFlags, value: &T, render_human: F) -> Result<(), String>
where
    T: Serialize,
    F: Fn(&T) -> String,
{
    if global.json {
        print_json(value)
    } else if global.ndjson {
        print_json_line(value)
    } else {
        println!("{}", render_human(value));
        Ok(())
    }
}

fn print_json<T>(value: &T) -> Result<(), String>
where
    T: Serialize + ?Sized,
{
    let text = serde_json::to_string_pretty(value)
        .map_err(|error| format!("failed to serialize JSON output: {error}"))?;
    println!("{text}");
    Ok(())
}

fn print_json_line<T>(value: &T) -> Result<(), String>
where
    T: Serialize + ?Sized,
{
    let text = serde_json::to_string(value)
        .map_err(|error| format!("failed to serialize output: {error}"))?;
    println!("{text}");
    Ok(())
}

fn print_ndjson<T>(values: &[T]) -> Result<(), String>
where
    T: Serialize,
{
    for value in values {
        print_json_line(value)?;
    }
    Ok(())
}

fn render_thread_summary(thread: &ThreadSummary) -> String {
    let title = thread.name.as_deref().unwrap_or("(unnamed)");
    let updated = thread.updated_at.unwrap_or(thread.created_at).to_rfc3339();
    format!("{}\t{}\t{}", thread.thread_id, title, updated)
}

fn render_thread_detail(detail: &ThreadDetail, include_turns: bool) -> String {
    let mut lines = vec![
        format!("thread_id: {}", detail.summary.thread_id),
        format!(
            "name: {}",
            detail.summary.name.as_deref().unwrap_or("(unnamed)")
        ),
        format!("created_at: {}", detail.summary.created_at.to_rfc3339()),
        format!("items: {}", detail.items_count),
        format!("commands: {}", detail.commands_count),
        format!("files_changed: {}", detail.files_changed_count),
    ];

    if let Some(cwd) = &detail.summary.cwd {
        lines.push(format!("cwd: {}", cwd.display()));
    }

    if include_turns {
        for turn in &detail.turns {
            lines.push(format!("turn {} [{}]", turn.turn_id, turn.status));
            for item in &turn.items {
                lines.push(format!("  - {}: {}", item.kind(), summarize_item(item)));
            }
        }
    }

    lines.join("\n")
}

fn summarize_item(item: &Item) -> String {
    match item {
        Item::UserMessage(message) | Item::AgentMessage(message) => {
            message.text.clone().unwrap_or_else(|| "(empty)".into())
        }
        Item::CommandExecution(command) => command
            .command
            .clone()
            .unwrap_or_else(|| "(command execution)".into()),
        Item::FileChange(change) => change
            .path
            .as_ref()
            .map(|path| path.display().to_string())
            .or_else(|| change.summary.clone())
            .unwrap_or_else(|| "(file change)".into()),
        Item::ReasoningSummary(summary) => {
            summary.text.clone().unwrap_or_else(|| "(summary)".into())
        }
        Item::WebSearch(search) => search
            .query
            .clone()
            .or_else(|| search.title.clone())
            .unwrap_or_else(|| "(web search)".into()),
        Item::McpToolCall(call) => call
            .tool
            .clone()
            .or_else(|| call.server.clone())
            .unwrap_or_else(|| "(mcp tool call)".into()),
        Item::Other(other) => format!("unknown item {}", other.kind),
    }
}

fn render_grep_match(entry: &GrepMatch) -> String {
    format!(
        "{}\t{}\t{}\t{}",
        entry.thread_id, entry.turn_id, entry.kind, entry.text
    )
}

fn render_doctor_report(report: &LocalDoctorReport) -> String {
    let mut lines = vec![
        format!("roots: {}", report.roots.len()),
        format!("parsed_threads: {}", report.parsed_threads),
        format!("malformed_files: {}", report.malformed_files),
        format!("malformed_lines: {}", report.malformed_lines),
    ];

    for root in &report.roots {
        lines.push(format!(
            "root [{}] {} (exists={}, session_files={})",
            root.source,
            root.path.display(),
            root.exists,
            root.session_files
        ));
    }

    for warning in &report.warnings {
        lines.push(format!("warning: {warning}"));
    }

    lines.join("\n")
}

fn render_search_result(entry: &SearchResult) -> String {
    let turn = entry.turn_id.as_deref().unwrap_or("-");
    format!(
        "{}\t{}\t{}\t{:.2}\t{}",
        entry.thread_id, turn, entry.kind, entry.score, entry.text
    )
}

fn render_index_build_report(report: &IndexBuildReport) -> String {
    [
        format!("path: {}", report.path.display()),
        format!("schema_version: {}", report.schema_version),
        format!("source_backend: {}", report.source_backend),
        format!("built_at: {}", report.built_at),
        format!("threads: {}", report.threads),
        format!("turns: {}", report.turns),
        format!("items: {}", report.items),
        format!("search_docs: {}", report.search_docs),
        format!("thread_manifest: {}", report.manifest_rows),
    ]
    .join("\n")
}

fn render_index_doctor_report(report: &IndexDoctorReport) -> String {
    let mut lines = vec![
        format!("path: {}", report.path.display()),
        format!("exists: {}", report.exists),
        format!("healthy: {}", report.healthy),
        format!(
            "schema_version: {}",
            report
                .schema_version
                .map(|value| value.to_string())
                .unwrap_or_else(|| "(missing)".into())
        ),
        format!(
            "schema_version_expected: {}",
            report.schema_version_expected
        ),
        format!("threads: {}", report.threads),
        format!("turns: {}", report.turns),
        format!("items: {}", report.items),
        format!("search_docs: {}", report.search_docs),
        format!("thread_manifest: {}", report.thread_manifest),
    ];

    for issue in &report.issues {
        lines.push(format!("issue: {issue}"));
    }

    lines.join("\n")
}

fn render_index_refresh_report(report: &IndexRefreshReport) -> String {
    [
        format!("path: {}", report.path.display()),
        format!("schema_version: {}", report.schema_version),
        format!("source_backend: {}", report.source_backend),
        format!("refreshed_at: {}", report.refreshed_at),
        format!("new_threads: {}", report.new_threads),
        format!("changed_threads: {}", report.changed_threads),
        format!("unchanged_threads: {}", report.unchanged_threads),
        format!("indexed_threads: {}", report.indexed_threads),
        format!("manifest_rows: {}", report.manifest_rows),
        format!(
            "watermark: {}",
            report.watermark.as_deref().unwrap_or("(none)")
        ),
    ]
    .join("\n")
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
    fn prints_command_help() {
        let parsed = Cli::parse(vec!["show".into(), "--help".into()]).expect("parse success");
        assert_eq!(parsed, ParseOutcome::PrintHelp(show_help()));

        let parsed = Cli::parse(vec!["--json".into(), "search".into(), "--help".into()])
            .expect("parse success");
        assert_eq!(parsed, ParseOutcome::PrintHelp(search_help()));

        let parsed = Cli::parse(vec!["index".into(), "drop".into(), "--help".into()])
            .expect("parse success");
        assert_eq!(
            parsed,
            ParseOutcome::PrintHelp(index_subcommand_help("drop").expect("drop help"))
        );

        let parsed = Cli::parse(vec!["export".into(), "--help".into()]).expect("parse success");
        assert_eq!(parsed, ParseOutcome::PrintHelp(export_help()));
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
    fn parses_show_include_turns() {
        let parsed = Cli::parse(vec![
            "show".into(),
            "--include-turns".into(),
            "thr_123".into(),
        ])
        .expect("parse success");
        let ParseOutcome::Run(cli) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            cli.command,
            Commands::Show {
                thread_id: "thr_123".into(),
                include_turns: true
            }
        );
    }

    #[test]
    fn parses_search_fresh_and_grep_regex() {
        let parsed = Cli::parse(vec!["search".into(), "sqlite".into(), "--fresh".into()])
            .expect("parse success");
        let ParseOutcome::Run(cli) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            cli.command,
            Commands::Search {
                query: "sqlite".into(),
                fresh: true
            }
        );

        let parsed = Cli::parse(vec!["grep".into(), "--regex".into(), "fatal.*".into()])
            .expect("parse success");
        let ParseOutcome::Run(cli) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            cli.command,
            Commands::Grep {
                pattern: "fatal.*".into(),
                regex: true
            }
        );
    }

    #[test]
    fn accepts_dash_prefixed_search_and_grep_positionals() {
        let parsed = Cli::parse(vec!["search".into(), "--helpful".into()]).expect("parse success");
        let ParseOutcome::Run(cli) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            cli.command,
            Commands::Search {
                query: "--helpful".into(),
                fresh: false
            }
        );

        let parsed = Cli::parse(vec!["grep".into(), "--regex".into(), "-foo.*".into()])
            .expect("parse success");
        let ParseOutcome::Run(cli) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            cli.command,
            Commands::Grep {
                pattern: "-foo.*".into(),
                regex: true
            }
        );
    }

    #[test]
    fn accepts_end_of_options_for_search_and_grep() {
        let parsed = Cli::parse(vec![
            "search".into(),
            "--fresh".into(),
            "--".into(),
            "--help".into(),
        ])
        .expect("parse success");
        let ParseOutcome::Run(cli) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            cli.command,
            Commands::Search {
                query: "--help".into(),
                fresh: true
            }
        );

        let parsed = Cli::parse(vec![
            "grep".into(),
            "--regex".into(),
            "--".into(),
            "--starts-with-dash".into(),
        ])
        .expect("parse success");
        let ParseOutcome::Run(cli) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            cli.command,
            Commands::Grep {
                pattern: "--starts-with-dash".into(),
                regex: true
            }
        );
    }

    #[test]
    fn parses_export_format_and_index_drop_yes() {
        let parsed = Cli::parse(vec![
            "export".into(),
            "thr_123".into(),
            "--format".into(),
            "markdown".into(),
        ])
        .expect("parse success");
        let ParseOutcome::Run(cli) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            cli.command,
            Commands::Export {
                thread_id: "thr_123".into(),
                format: "markdown".into()
            }
        );

        let parsed =
            Cli::parse(vec!["index".into(), "drop".into(), "--yes".into()]).expect("parse success");
        let ParseOutcome::Run(cli) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            cli.command,
            Commands::Index(IndexCommands::Drop { yes: true })
        );
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
    fn rejects_invalid_export_format() {
        let error = Cli::parse(vec![
            "export".into(),
            "thr_123".into(),
            "--format".into(),
            "html".into(),
        ])
        .expect_err("parse should fail");
        assert_eq!(
            error,
            "invalid export format `html`; expected json|markdown|prompt-pack"
        );
    }

    #[test]
    fn rejects_unknown_command_options_and_duplicate_flags() {
        let error = Cli::parse(vec!["search".into(), "--bogus".into(), "query".into()])
            .expect_err("parse should fail");
        assert_eq!(error, "unknown option: --bogus");

        let error = Cli::parse(vec![
            "show".into(),
            "--include-turns".into(),
            "--include-turns".into(),
            "thr_123".into(),
        ])
        .expect_err("parse should fail");
        assert_eq!(error, "duplicate option: --include-turns");

        let error = Cli::parse(vec![
            "index".into(),
            "drop".into(),
            "--yes".into(),
            "--yes".into(),
        ])
        .expect_err("parse should fail");
        assert_eq!(error, "duplicate option: --yes");
    }

    #[test]
    fn rejects_missing_required_arguments_with_flags_present() {
        let error = Cli::parse(vec!["show".into(), "--include-turns".into()])
            .expect_err("parse should fail");
        assert_eq!(error, "missing required argument: show <thread-id>");

        let error =
            Cli::parse(vec!["search".into(), "--fresh".into()]).expect_err("parse should fail");
        assert_eq!(error, "missing required argument: search <query>");

        let error =
            Cli::parse(vec!["grep".into(), "--regex".into()]).expect_err("parse should fail");
        assert_eq!(error, "missing required argument: grep <pattern>");
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
