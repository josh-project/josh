use serde::Serialize;

use crate::cli_println as println;

#[derive(Debug, clap::Parser)]
pub struct CapabilitiesArgs {
    /// Return only the minimum information needed for agent negotiation
    #[arg(long)]
    pub brief: bool,
}

#[derive(Debug, Serialize)]
struct OutputCapabilities {
    formats: [&'static str; 3],
    schema_versions: [&'static str; 1],
    json_errors: bool,
    jsonl_events: bool,
    machine_results_always_use_stdout: bool,
    compact_json_by_default: bool,
    data_only_with_quiet: bool,
}

#[derive(Debug, Serialize)]
struct AutomationCapabilities {
    non_interactive: bool,
    quiet: bool,
    no_progress: bool,
    color_control: bool,
    stable_error_codes: bool,
    installable_agent_skill: bool,
}

#[derive(Debug, Serialize)]
struct Capabilities {
    version: &'static str,
    output: OutputCapabilities,
    automation: AutomationCapabilities,
    commands: [&'static str; 16],
}

#[derive(Debug, Serialize)]
struct BriefCapabilities {
    version: &'static str,
    schema_version: &'static str,
    output_formats: [&'static str; 2],
    agent_skill: bool,
    workspaces: bool,
}

pub fn handle_capabilities(args: &CapabilitiesArgs) -> anyhow::Result<()> {
    if args.brief {
        let capabilities = BriefCapabilities {
            version: josh_core::VERSION,
            schema_version: crate::output::SCHEMA_VERSION,
            output_formats: ["json", "jsonl"],
            agent_skill: true,
            workspaces: true,
        };
        crate::output::set_data(&capabilities)?;
        println!(
            "Josh {} schema={} output=json,jsonl agent-skill=yes workspaces=yes",
            capabilities.version, capabilities.schema_version
        );
        return Ok(());
    }

    let capabilities = Capabilities {
        version: josh_core::VERSION,
        output: OutputCapabilities {
            formats: ["human", "json", "jsonl"],
            schema_versions: [crate::output::SCHEMA_VERSION],
            json_errors: true,
            jsonl_events: true,
            machine_results_always_use_stdout: true,
            compact_json_by_default: true,
            data_only_with_quiet: true,
        },
        automation: AutomationCapabilities {
            non_interactive: true,
            quiet: true,
            no_progress: true,
            color_control: true,
            stable_error_codes: true,
            installable_agent_skill: true,
        },
        commands: [
            "agent",
            "auth",
            "cache",
            "capabilities",
            "changes",
            "clone",
            "compose",
            "completions",
            "fetch",
            "filter",
            "link",
            "pull",
            "push",
            "remote",
            "status",
            "workspace",
        ],
    };
    crate::output::set_data(&capabilities)?;

    println!("Josh {}", capabilities.version);
    println!("Output formats: human, json, jsonl");
    println!("Machine schema version: {}", crate::output::SCHEMA_VERSION);
    println!("Non-interactive mode: supported");
    println!("Workspace management: supported");
    println!("Agent skill: installable");
    Ok(())
}
