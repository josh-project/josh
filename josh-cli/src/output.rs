use std::io::Write;
use std::sync::{LazyLock, Mutex};

use clap::ValueEnum;
use serde::Serialize;
use serde_json::{Value, json};

pub const SCHEMA_VERSION: &str = "1";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum OutputFormat {
    #[default]
    Human,
    Json,
    Jsonl,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "lower")]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Debug)]
pub struct OutputOptions {
    pub format: OutputFormat,
    pub color: ColorChoice,
    pub pretty: bool,
    pub quiet: bool,
    pub no_progress: bool,
    pub non_interactive: bool,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            format: OutputFormat::Human,
            color: ColorChoice::Auto,
            pretty: false,
            quiet: false,
            no_progress: false,
            non_interactive: false,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct Message {
    level: &'static str,
    message: String,
}

#[derive(Debug, Default)]
struct OutputState {
    options: OutputOptions,
    command: String,
    messages: Vec<Message>,
    data: Option<Value>,
    initialized: bool,
}

static STATE: LazyLock<Mutex<OutputState>> = LazyLock::new(|| Mutex::new(OutputState::default()));

pub fn init(options: OutputOptions, command: impl Into<String>) {
    let mut state = STATE.lock().expect("CLI output state lock poisoned");
    *state = OutputState {
        options,
        command: command.into(),
        messages: Vec::new(),
        data: None,
        initialized: true,
    };
}

pub fn format() -> OutputFormat {
    STATE
        .lock()
        .expect("CLI output state lock poisoned")
        .options
        .format
}

pub fn is_machine() -> bool {
    format() != OutputFormat::Human
}

pub fn is_non_interactive() -> bool {
    let state = STATE.lock().expect("CLI output state lock poisoned");
    state.options.non_interactive || state.options.format != OutputFormat::Human
}

pub fn stdout(message: impl Into<String>) {
    let message = message.into();
    let mut state = STATE.lock().expect("CLI output state lock poisoned");

    if !state.initialized || state.options.format == OutputFormat::Human {
        let _ = writeln!(std::io::stdout().lock(), "{message}");
    } else if !state.options.quiet {
        state.messages.push(Message {
            level: "result",
            message,
        });
    }
}

pub fn stderr(message: impl Into<String>) {
    let message = message.into();
    let mut state = STATE.lock().expect("CLI output state lock poisoned");
    let warning = message.starts_with("Warning:") || message.starts_with("warning:");

    if !state.initialized || state.options.format == OutputFormat::Human {
        if !state.options.quiet || warning {
            let _ = writeln!(std::io::stderr().lock(), "{message}");
        }
    } else if !state.options.quiet {
        state.messages.push(Message {
            level: if warning { "warning" } else { "diagnostic" },
            message,
        });
    }
}

pub fn raw_stdout(value: &str) -> std::io::Result<()> {
    std::io::stdout().lock().write_all(value.as_bytes())
}

pub fn sanitize(value: &str) -> String {
    if let Ok(test_root) = std::env::var("TESTTMP") {
        value.replace(&test_root, "${TESTTMP}")
    } else {
        value.to_string()
    }
}

pub fn warning(message: impl Into<String>) {
    let message = message.into();
    let message = if message.starts_with("Warning:") {
        message
    } else {
        format!("Warning: {message}")
    };
    stderr(message);
}

pub fn set_data<T: Serialize>(data: &T) -> anyhow::Result<()> {
    set_data_value(serde_json::to_value(data)?);
    Ok(())
}

pub fn set_data_value(data: Value) {
    STATE.lock().expect("CLI output state lock poisoned").data = Some(data);
}

fn error_text(error: &anyhow::Error) -> String {
    error
        .chain()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase()
}

fn hints_for(error: &anyhow::Error) -> Vec<String> {
    let message = error_text(error);
    let top = error.to_string().to_lowercase();
    let mut hints = Vec::new();

    if message.contains("not in a git repository") {
        hints.push("Run this command inside a Git working tree.".to_string());
    }
    if top.contains("remote") && message.contains("not found") && !top.contains("--base") {
        hints.push("Run 'josh remote list' to see configured remotes.".to_string());
    }
    if top.contains("workspace") && message.contains("no such file") {
        hints.push("Run 'josh workspace list' to see discovered workspaces.".to_string());
    }
    if message.contains("authentication") || message.contains("log in") {
        hints.push("Run 'josh auth login github' or configure a token.".to_string());
    }

    hints
}

fn error_code(error: &anyhow::Error) -> &'static str {
    let message = error_text(error);
    let top = error.to_string().to_lowercase();
    if message.contains("not in a git repository") {
        "repository.not_found"
    } else if message.contains("authentication") || message.contains("log in") {
        "authentication.required"
    } else if top.contains("--base") && message.contains("not found") {
        "reference.not_found"
    } else if message.contains("remote") && message.contains("not found") {
        "remote.not_found"
    } else if top.contains("filter")
        && (message.contains("failed to parse") || message.contains("invalid"))
    {
        "filter.invalid"
    } else if top.contains("workspace") && message.contains("no such file") {
        "workspace.not_found"
    } else if top.contains("agent skill") && message.contains("already exists") {
        "agent_skill.already_exists"
    } else if top.contains("checkout destination") && message.contains("already exists") {
        "workspace.destination_exists"
    } else if top.contains("clone destination") && message.contains("already exists") {
        "clone.destination_exists"
    } else if top.contains("workspace") && message.contains("already exists") {
        "workspace.already_exists"
    } else if message.contains("invalid workspace") || top.contains("workspace") {
        "workspace.invalid"
    } else if message.contains("failed to parse") || message.contains("invalid filter") {
        "filter.invalid"
    } else if message.contains("command exited with code") {
        "git.command_failed"
    } else {
        "josh.command_failed"
    }
}

fn add_messages(value: &mut Value, state: &OutputState) {
    if !state.options.quiet {
        value
            .as_object_mut()
            .expect("result envelope is an object")
            .insert(
                "messages".to_string(),
                serde_json::to_value(&state.messages)
                    .expect("serializing CLI messages cannot fail"),
            );
    }
}

fn print_json(value: &Value, pretty: bool, stderr: bool) {
    let rendered = if pretty {
        serde_json::to_string_pretty(value)
    } else {
        serde_json::to_string(value)
    }
    .expect("serializing CLI output cannot fail");

    if stderr {
        let _ = writeln!(std::io::stderr().lock(), "{rendered}");
    } else {
        let _ = writeln!(std::io::stdout().lock(), "{rendered}");
    }
}

pub fn finish_success() {
    let state = STATE.lock().expect("CLI output state lock poisoned");
    match state.options.format {
        OutputFormat::Human => {}
        OutputFormat::Json => {
            let mut result = json!({
                "schema_version": SCHEMA_VERSION,
                "type": "result",
                "command": state.command,
                "success": true,
                "data": state.data,
            });
            add_messages(&mut result, &state);
            print_json(&result, state.options.pretty, false);
        }
        OutputFormat::Jsonl => {
            for message in &state.messages {
                print_json(
                    &json!({
                        "schema_version": SCHEMA_VERSION,
                        "type": "message",
                        "command": state.command,
                        "level": message.level,
                        "message": message.message,
                    }),
                    false,
                    false,
                );
            }
            print_json(
                &json!({
                    "schema_version": SCHEMA_VERSION,
                    "type": "result",
                    "command": state.command,
                    "success": true,
                    "data": state.data,
                }),
                false,
                false,
            );
        }
    }
}

pub fn finish_error(error: &anyhow::Error) {
    let state = STATE.lock().expect("CLI output state lock poisoned");
    let causes: Vec<_> = error.chain().skip(1).map(ToString::to_string).collect();
    let hints = hints_for(error);

    match state.options.format {
        OutputFormat::Human => {
            let mut stderr = std::io::stderr().lock();
            let _ = writeln!(stderr, "Error: {error}");
            if !causes.is_empty() {
                let _ = writeln!(stderr, "Caused by:");
                for cause in &causes {
                    let _ = writeln!(stderr, "  {cause}");
                }
            }
            for hint in &hints {
                let _ = writeln!(stderr, "Hint: {hint}");
            }
        }
        OutputFormat::Json => {
            let mut result = json!({
                "schema_version": SCHEMA_VERSION,
                "type": "result",
                "command": state.command,
                "success": false,
                "data": state.data,
                "error": {
                    "code": error_code(error),
                    "message": error.to_string(),
                    "causes": causes,
                    "hints": hints,
                },
            });
            add_messages(&mut result, &state);
            print_json(&result, state.options.pretty, false);
        }
        OutputFormat::Jsonl => {
            for message in &state.messages {
                print_json(
                    &json!({
                        "schema_version": SCHEMA_VERSION,
                        "type": "message",
                        "command": state.command,
                        "level": message.level,
                        "message": message.message,
                    }),
                    false,
                    false,
                );
            }
            print_json(
                &json!({
                    "schema_version": SCHEMA_VERSION,
                    "type": "result",
                    "command": state.command,
                    "success": false,
                    "data": state.data,
                    "error": {
                        "code": error_code(error),
                        "message": error.to_string(),
                        "causes": causes,
                        "hints": hints,
                    },
                }),
                false,
                false,
            );
        }
    }
}

pub fn detect_pretty(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--pretty")
}

pub fn detect_format(args: &[String]) -> OutputFormat {
    let mut format = std::env::var("JOSH_OUTPUT")
        .ok()
        .and_then(|value| match value.as_str() {
            "json" => Some(OutputFormat::Json),
            "jsonl" => Some(OutputFormat::Jsonl),
            "human" => Some(OutputFormat::Human),
            _ => None,
        })
        .unwrap_or_default();

    for (index, arg) in args.iter().enumerate() {
        if let Some(value) = arg.strip_prefix("--output=") {
            format = match value {
                "json" => OutputFormat::Json,
                "jsonl" => OutputFormat::Jsonl,
                _ => format,
            };
        } else if arg == "--output"
            && let Some(value) = args.get(index + 1)
        {
            format = match value.as_str() {
                "json" => OutputFormat::Json,
                "jsonl" => OutputFormat::Jsonl,
                _ => format,
            };
        }
    }

    format
}

fn command_from_help(message: &str) -> String {
    let Some(usage) = message
        .lines()
        .find_map(|line| line.trim().strip_prefix("Usage: josh "))
    else {
        return "josh".to_string();
    };
    let parts = usage
        .split_whitespace()
        .take_while(|part| !part.starts_with('<') && !part.starts_with('['))
        .filter(|part| !part.starts_with('-'))
        .collect::<Vec<_>>();
    if parts.is_empty() {
        "josh".to_string()
    } else {
        parts.join(".")
    }
}

pub fn render_clap(
    message: &str,
    format: OutputFormat,
    pretty: bool,
    success: bool,
    exit_code: i32,
) {
    if format == OutputFormat::Human {
        if success {
            let _ = write!(std::io::stdout().lock(), "{message}");
        } else {
            let _ = write!(std::io::stderr().lock(), "{message}");
        }
        return;
    }

    let value = json!({
        "schema_version": SCHEMA_VERSION,
        "type": if success { "help" } else { "result" },
        "command": command_from_help(message),
        "success": success,
        "data": if success { Some(json!({ "text": message })) } else { None },
        "error": if success {
            None
        } else {
            Some(json!({
                "code": "cli.usage",
                "message": message.trim(),
                "causes": [],
                "hints": ["Run 'josh --help' to inspect the command syntax."],
                "exit_code": exit_code,
            }))
        },
    });
    print_json(&value, format == OutputFormat::Json && pretty, false);
}

#[macro_export]
macro_rules! cli_println {
    () => {
        $crate::output::stdout(String::new())
    };
    ($($arg:tt)*) => {
        $crate::output::stdout(format!($($arg)*))
    };
}

#[macro_export]
macro_rules! cli_eprintln {
    () => {
        $crate::output::stderr(String::new())
    };
    ($($arg:tt)*) => {
        $crate::output::stderr(format!($($arg)*))
    };
}
