use josh::{josh_error, JoshResult};

pub struct Remote {
    pub http: Option<String>,
    pub ssh: Option<String>,
}

pub struct Args {
    pub remote: Remote,
    pub local: String,
    pub poll_user: Option<String>,
    pub gc: bool,
    pub require_auth: bool,
    pub no_background: bool,
    pub port: u16,
    pub cache_duration: u64,
    pub static_resource_proxy_target: Option<String>,
    pub filter_prefix: Option<String>,
}

fn parse_int<T: std::str::FromStr>(
    matches: &clap::ArgMatches,
    arg_name: &str,
    default: Option<T>,
) -> JoshResult<T>
where
    <T as std::str::FromStr>::Err: std::fmt::Display,
{
    let arg = matches.get_one::<String>(arg_name).map(|s| s.as_str());

    let arg = match (arg, default) {
        (None, None) => {
            return Err(josh_error(&format!(
                "missing required argument: {}",
                arg_name
            )))
        }
        (None, Some(default)) => Ok(default),
        (Some(value), _) => value.parse::<T>(),
    };

    arg.map_err(|e| josh_error(&format!("error parsing argument {}: {}", arg_name, e)))
}

fn make_command() -> clap::Command {
    clap::Command::new("josh-proxy")
        .arg(
            clap::Arg::new("remote")
                .long("remote")
                .action(clap::ArgAction::Append),
        )
        .arg(clap::Arg::new("local").long("local"))
        .arg(clap::Arg::new("poll").long("poll"))
        .arg(
            clap::Arg::new("gc")
                .long("gc")
                .action(clap::ArgAction::SetTrue)
                .help("Run git gc during maintenance"),
        )
        .arg(
            clap::Arg::new("require-auth")
                .long("require-auth")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("no-background")
                .long("no-background")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(clap::Arg::new("n").short('n').help(
            "DEPRECATED - no effect! Number of concurrent upstream git fetch/push operations",
        ))
        .arg(clap::Arg::new("port").long("port"))
        .arg(
            clap::Arg::new("cache-duration")
                .long("cache-duration")
                .short('c')
                .help("Duration between forced cache refresh"),
        )
        .arg(
            clap::Arg::new("static-resource-proxy-target")
                .long("static-resource-proxy-target")
                .help("Proxy static resource requests to a different URL"),
        )
        .arg(
            clap::Arg::new("filter-prefix")
                .long("filter-prefix")
                .help("Filter to be prefixed to all queries of this instance"),
        )
}

fn parse_remotes(values: &[String]) -> JoshResult<Remote> {
    let mut result = Remote {
        http: None,
        ssh: None,
    };

    for value in values {
        match value {
            v if v.starts_with("http://") || v.starts_with("https://") => {
                result.http = match result.http {
                    None => Some(v.clone()),
                    Some(v) => return Err(josh_error(&format!("HTTP remote already set: {}", v))),
                };
            }
            v if v.starts_with("ssh://") => {
                result.ssh = match result.ssh {
                    None => Some(v.clone()),
                    Some(v) => return Err(josh_error(&format!("SSH remote already set: {}", v))),
                };
            }
            _ => {
                return Err(josh_error(&format!(
                    "Unsupported remote protocol: {}",
                    value
                )))
            }
        }
    }

    Ok(result)
}

pub fn parse_args() -> josh::JoshResult<Args> {
    let args = make_command().get_matches_from(std::env::args());

    let remote = args
        .get_many::<String>("remote")
        .ok_or(josh_error("no remote specified"))?
        .cloned()
        .collect::<Vec<_>>();
    let remote = parse_remotes(&remote)?;

    let local = args
        .get_one::<String>("local")
        .ok_or(josh_error("missing local directory"))?
        .clone();

    let poll_user = args.get_one::<String>("poll").map(String::clone);
    let port = parse_int::<u16>(&args, "port", Some(8000))?;
    let cache_duration = parse_int::<u64>(&args, "cache-duration", Some(0))?;
    let static_resource_proxy_target = args
        .get_one::<String>("static-resource-proxy-target")
        .map(String::clone);

    let filter_prefix = args.get_one::<String>("filter-prefix").map(String::clone);

    Ok(Args {
        remote,
        local,
        poll_user,
        gc: args.get_flag("gc"),
        require_auth: args.get_flag("require-auth"),
        no_background: args.get_flag("no-background"),
        port,
        cache_duration,
        static_resource_proxy_target,
        filter_prefix,
    })
}

pub fn parse_args_or_exit(code: i32) -> Args {
    match parse_args() {
        Err(e) => {
            eprintln!("Argument parsing error: {}", e.0);
            std::process::exit(code);
        }
        Ok(args) => args,
    }
}

#[test]
fn verify_app() {
    make_command().debug_assert();
}
