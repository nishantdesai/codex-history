use std::collections::HashMap;

use regex::Regex;
use serde::Serialize;

use crate::backend::local::{
    load_session_index_names, GrepMatch, GrepReport, LocalBackend, LocalDoctorReport,
};
use crate::index::ingest::{
    build_local_index, load_manifest_snapshot, refresh_local_index, IndexBuildReport,
    IndexRefreshReport,
};
use crate::index::manifest::default_index_path;
use crate::index::query::{
    load_index_thread_info, search_index, search_with_fresh_overlay, IndexedThreadInfo,
    SearchResult,
};
use crate::index::schema::{doctor as doctor_index, IndexDoctorReport};
use crate::model::{
    render_thread_export, ExportDocument, ExportFormat, Item, ThreadDetail, ThreadSummary,
};
use crate::redact::{redact_human_text, to_redacted_json_string};
use crate::search_scope::SearchScope;

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
        include_thinking: bool,
        include_tools: bool,
    },
    Grep {
        pattern: String,
        regex: bool,
        include_thinking: bool,
        include_tools: bool,
    },
    Export {
        thread_id: String,
        format: ExportFormat,
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
                let Commands::Search {
                    query,
                    fresh,
                    include_thinking,
                    include_tools,
                } = &self.command
                else {
                    unreachable!("matched command variant");
                };
                let scope = SearchScope {
                    include_thinking: *include_thinking,
                    include_tools: *include_tools,
                };
                let path = default_index_path();
                let (results, thread_info) = if *fresh {
                    let manifest = load_manifest_snapshot(&path)?;
                    let details = backend.list_thread_details()?;
                    let results =
                        search_with_fresh_overlay(&path, query, 50, scope, &details, &manifest)?;
                    let info = thread_display_info_from_details(&details);
                    (results, info)
                } else {
                    let results = search_index(&path, query, 50, scope)?;
                    let ids = unique_thread_ids_from_search_results(&results);
                    let info = thread_display_info_from_index(&path, &ids)?;
                    (results, info)
                };
                if self.global.json || self.global.ndjson {
                    render_collection(&self.global, &results, render_search_result)
                } else {
                    render_search_results_human(&results, query, &thread_info)
                }
            }
            Commands::Grep {
                pattern,
                regex,
                include_thinking,
                include_tools,
            } => {
                let scope = SearchScope {
                    include_thinking: *include_thinking,
                    include_tools: *include_tools,
                };
                let report = backend.grep_report(pattern, *regex, scope)?;
                if self.global.json || self.global.ndjson {
                    render_collection(&self.global, &report.matches, render_grep_match)
                } else {
                    render_grep_matches_human(&report, pattern, *regex)
                }
            }
            Commands::Export { thread_id, format } => {
                let detail = backend
                    .show_thread(thread_id, true)?
                    .ok_or_else(|| format!("thread not found: {thread_id}"))?;
                render_export(&self.global, *format, &detail)
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
    let mut include_thinking = false;
    let mut include_tools = false;
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
            "--include-thinking" => {
                include_thinking = set_flag(include_thinking, "--include-thinking")?
            }
            "--include-tools" => include_tools = set_flag(include_tools, "--include-tools")?,
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
            include_thinking,
            include_tools,
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
    let mut include_thinking = false;
    let mut include_tools = false;
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
            "--include-thinking" => {
                include_thinking = set_flag(include_thinking, "--include-thinking")?
            }
            "--include-tools" => include_tools = set_flag(include_tools, "--include-tools")?,
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
            include_thinking,
            include_tools,
        },
        consumed: args.len(),
    }))
}

fn parse_export(args: &[String]) -> Result<ParsedCommand, String> {
    let mut thread_id = None;
    let mut format = ExportFormat::Json;
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
                format = parse_export_format(value)?;
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

fn parse_export_format(value: &str) -> Result<ExportFormat, String> {
    value.parse()
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
    "codex-history search\nSearch across history\n\nUSAGE:\n  codex-history [OPTIONS] search [--fresh] [--include-thinking] [--include-tools] <query>\n\nOPTIONS:\n  --fresh\n  --include-thinking\n  --include-tools\n  -h, --help"
        .to_string()
}

fn grep_help() -> String {
    "codex-history grep\nLiteral or regex transcript search without ranking\n\nUSAGE:\n  codex-history [OPTIONS] grep [--regex] [--include-thinking] [--include-tools] <pattern>\n\nOPTIONS:\n  --regex\n  --include-thinking\n  --include-tools\n  -h, --help"
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
            println!("{}", redact_human_text(&render_human(value)));
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
        println!("{}", redact_human_text(&render_human(value)));
        Ok(())
    }
}

fn print_json<T>(value: &T) -> Result<(), String>
where
    T: Serialize + ?Sized,
{
    let text = to_redacted_json_string(value, true)?;
    println!("{text}");
    Ok(())
}

fn print_json_line<T>(value: &T) -> Result<(), String>
where
    T: Serialize + ?Sized,
{
    let text = to_redacted_json_string(value, false)?;
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

fn render_export(
    global: &GlobalFlags,
    format: ExportFormat,
    detail: &ThreadDetail,
) -> Result<(), String> {
    match format {
        ExportFormat::Json => {
            let document = ExportDocument::new(format, detail.clone());
            if global.ndjson {
                print_json_line(&document)
            } else {
                print_json(&document)
            }
        }
        ExportFormat::Markdown | ExportFormat::PromptPack => {
            if global.json || global.ndjson {
                return Err(format!(
                    "cannot combine {} with `export --format {format}`",
                    if global.ndjson { "--ndjson" } else { "--json" }
                ));
            }

            let rendered = render_thread_export(format, detail)?;
            println!("{}", redact_human_text(&rendered));
            Ok(())
        }
    }
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

fn render_grep_matches_human(
    report: &GrepReport,
    pattern: &str,
    regex: bool,
) -> Result<(), String> {
    let groups = group_grep_entries(&report.matches, pattern, regex)?;
    let threads = thread_display_info_from_summaries(&report.thread_summaries);

    for (index, group) in groups.iter().enumerate() {
        if index > 0 {
            println!();
        }
        print_thread_group(index + 1, group, &threads, None);
    }
    Ok(())
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

fn render_search_results_human(
    entries: &[SearchResult],
    query: &str,
    threads: &HashMap<String, ThreadDisplayInfo>,
) -> Result<(), String> {
    let groups = group_search_entries(entries, query);

    for (index, group) in groups.iter().enumerate() {
        if index > 0 {
            println!();
        }
        print_thread_group(index + 1, group, threads, Some(group.best_score));
    }
    Ok(())
}

fn human_kind_label(kind: &str) -> &str {
    match kind {
        "user_message" => "user",
        "agent_message" => "assistant",
        "command_execution" => "command",
        "file_change" => "file",
        "reasoning_summary" => "reasoning",
        "thread_name" => "thread name",
        "thread_preview" => "thread preview",
        other => other,
    }
}

fn compact_preview(text: &str) -> String {
    const MAX_CHARS: usize = 160;

    let compact = text
        .split_whitespace()
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    let mut preview = String::new();
    for (index, ch) in compact.chars().enumerate() {
        if index >= MAX_CHARS {
            preview.push_str("...");
            return preview;
        }
        preview.push(ch);
    }

    preview
}

fn thread_display_info_from_summaries(
    threads: &HashMap<String, ThreadSummary>,
) -> HashMap<String, ThreadDisplayInfo> {
    threads
        .iter()
        .map(|(thread_id, summary)| {
            (
                thread_id.clone(),
                ThreadDisplayInfo {
                    name: summary.name.clone(),
                    preview: summary.preview.clone(),
                    cwd: summary.cwd.as_ref().map(|path| path.display().to_string()),
                },
            )
        })
        .collect()
}

fn thread_display_info_from_details(
    details: &[ThreadDetail],
) -> HashMap<String, ThreadDisplayInfo> {
    details
        .iter()
        .map(|detail| {
            (
                detail.summary.thread_id.clone(),
                ThreadDisplayInfo {
                    name: detail.summary.name.clone(),
                    preview: detail.summary.preview.clone(),
                    cwd: detail
                        .summary
                        .cwd
                        .as_ref()
                        .map(|path| path.display().to_string()),
                },
            )
        })
        .collect()
}

fn unique_thread_ids_from_search_results(entries: &[SearchResult]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut ids = Vec::new();
    for entry in entries {
        if seen.insert(entry.thread_id.clone()) {
            ids.push(entry.thread_id.clone());
        }
    }
    ids
}

fn thread_display_info_from_index(
    path: &std::path::Path,
    thread_ids: &[String],
) -> Result<HashMap<String, ThreadDisplayInfo>, String> {
    let indexed = load_index_thread_info(path, thread_ids)?;
    let session_names = load_session_index_names().unwrap_or_default();
    let mut info = HashMap::new();

    for thread_id in thread_ids {
        let indexed_info = indexed.get(thread_id);
        let mut display = indexed_info
            .map(thread_display_info_from_indexed)
            .unwrap_or_default();
        if display.name.is_none() {
            display.name = session_names.get(thread_id).cloned();
        }
        info.insert(thread_id.clone(), display);
    }

    Ok(info)
}

fn thread_display_info_from_indexed(info: &IndexedThreadInfo) -> ThreadDisplayInfo {
    ThreadDisplayInfo {
        name: info.name.clone(),
        preview: info.preview.clone(),
        cwd: info.cwd.clone(),
    }
}

#[derive(Debug, Clone, Default)]
struct ThreadDisplayInfo {
    name: Option<String>,
    preview: Option<String>,
    cwd: Option<String>,
}

#[derive(Debug, Clone)]
struct ThreadResultGroup {
    thread_id: String,
    hits: usize,
    occurrences: usize,
    kinds: Vec<String>,
    preview: String,
    best_score: f64,
}

fn group_grep_entries(
    entries: &[GrepMatch],
    pattern: &str,
    regex: bool,
) -> Result<Vec<ThreadResultGroup>, String> {
    let matcher = if regex {
        Some(Regex::new(pattern).map_err(|error| format!("invalid regex `{pattern}`: {error}"))?)
    } else {
        None
    };
    let terms = query_terms(pattern);
    let mut groups = Vec::new();
    let mut positions = HashMap::new();

    for entry in entries {
        let position = if let Some(position) = positions.get(&entry.thread_id).copied() {
            position
        } else {
            let position = groups.len();
            positions.insert(entry.thread_id.clone(), position);
            groups.push(ThreadResultGroup {
                thread_id: entry.thread_id.clone(),
                hits: 0,
                occurrences: 0,
                kinds: Vec::new(),
                preview: compact_preview(&entry.text),
                best_score: 0.0,
            });
            position
        };

        let group = &mut groups[position];
        group.hits += 1;
        group.occurrences += grep_occurrences(&entry.text, pattern, matcher.as_ref(), &terms);
        push_unique_kind(&mut group.kinds, &entry.kind);
    }

    Ok(groups)
}

fn group_search_entries(entries: &[SearchResult], query: &str) -> Vec<ThreadResultGroup> {
    let terms = query_terms(query);
    let mut groups = Vec::new();
    let mut positions = HashMap::new();

    for entry in entries {
        let position = if let Some(position) = positions.get(&entry.thread_id).copied() {
            position
        } else {
            let position = groups.len();
            positions.insert(entry.thread_id.clone(), position);
            groups.push(ThreadResultGroup {
                thread_id: entry.thread_id.clone(),
                hits: 0,
                occurrences: 0,
                kinds: Vec::new(),
                preview: compact_preview(&entry.text),
                best_score: entry.score,
            });
            position
        };

        let group = &mut groups[position];
        group.hits += 1;
        group.occurrences += text_term_occurrences(&entry.text, &terms, query);
        if entry.score > group.best_score {
            group.best_score = entry.score;
            group.preview = compact_preview(&entry.text);
        }
        push_unique_kind(&mut group.kinds, &entry.kind);
    }

    groups
}

fn print_thread_group(
    rank: usize,
    group: &ThreadResultGroup,
    threads: &HashMap<String, ThreadDisplayInfo>,
    best_score: Option<f64>,
) {
    println!(
        "{}. thread_id: {}",
        rank,
        redact_human_text(&group.thread_id)
    );
    if let Some(summary) = threads.get(&group.thread_id) {
        if let Some(name) = summary
            .name
            .as_deref()
            .filter(|name| !name.trim().is_empty())
        {
            println!("   name: {}", redact_human_text(name));
        } else if let Some(prompt) = summary
            .preview
            .as_deref()
            .filter(|prompt| !prompt.trim().is_empty())
        {
            println!(
                "   first prompt: {}",
                redact_human_text(&compact_preview(prompt))
            );
        }
        if let Some(cwd) = &summary.cwd {
            println!("   cwd: {}", redact_human_text(cwd));
        }
    }
    println!("   hits: {}", group.hits);
    println!("   occurrences: {}", group.occurrences);
    println!(
        "   matched in: {}",
        redact_human_text(
            &group
                .kinds
                .iter()
                .map(|kind| human_kind_label(kind).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    );
    if let Some(score) = best_score {
        println!("   best score: {:.2}", score);
    }
    println!("   preview: {}", redact_human_text(&group.preview));
}

fn push_unique_kind(kinds: &mut Vec<String>, kind: &str) {
    if kinds.iter().any(|existing| existing == kind) {
        return;
    }
    kinds.push(kind.to_string());
}

fn grep_occurrences(text: &str, pattern: &str, regex: Option<&Regex>, terms: &[String]) -> usize {
    if let Some(regex) = regex {
        let count = regex.find_iter(text).count();
        return count.max(1);
    }

    text_term_occurrences(text, terms, pattern)
}

fn text_term_occurrences(text: &str, terms: &[String], fallback: &str) -> usize {
    if !terms.is_empty() {
        let tokens = query_terms(text);
        let count = tokens
            .iter()
            .filter(|token| terms.iter().any(|term| term == *token))
            .count();
        if count > 0 {
            return count;
        }
    }

    count_substring_case_insensitive(text, fallback).max(1)
}

fn count_substring_case_insensitive(text: &str, needle: &str) -> usize {
    let needle = needle.trim();
    if needle.is_empty() {
        return 0;
    }

    let text = text.to_ascii_lowercase();
    let needle = needle.to_ascii_lowercase();
    let mut count = 0;
    let mut offset = 0;

    while let Some(index) = text[offset..].find(&needle) {
        count += 1;
        offset += index + needle.len();
    }

    count
}

fn query_terms(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in query.chars() {
        if ch.is_alphanumeric() {
            current.extend(ch.to_lowercase());
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
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
                fresh: true,
                include_thinking: false,
                include_tools: false,
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
                regex: true,
                include_thinking: false,
                include_tools: false,
            }
        );
    }

    #[test]
    fn parses_search_and_grep_scope_flags() {
        let parsed = Cli::parse(vec![
            "search".into(),
            "--include-thinking".into(),
            "--include-tools".into(),
            "sqlite".into(),
        ])
        .expect("parse success");
        let ParseOutcome::Run(cli) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            cli.command,
            Commands::Search {
                query: "sqlite".into(),
                fresh: false,
                include_thinking: true,
                include_tools: true,
            }
        );

        let parsed = Cli::parse(vec![
            "grep".into(),
            "--include-tools".into(),
            "build".into(),
        ])
        .expect("parse success");
        let ParseOutcome::Run(cli) = parsed else {
            panic!("expected run");
        };
        assert_eq!(
            cli.command,
            Commands::Grep {
                pattern: "build".into(),
                regex: false,
                include_thinking: false,
                include_tools: true,
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
                fresh: false,
                include_thinking: false,
                include_tools: false,
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
                regex: true,
                include_thinking: false,
                include_tools: false,
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
                fresh: true,
                include_thinking: false,
                include_tools: false,
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
                regex: true,
                include_thinking: false,
                include_tools: false,
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
                format: ExportFormat::Markdown
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
