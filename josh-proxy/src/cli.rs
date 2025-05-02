#[derive(Clone, Debug)]
pub enum Remote {
    Http(String),
    Ssh(String),
}

fn parse_remote(s: &str) -> Result<Remote, &'static str> {
    match s {
        s if s.starts_with("http://") || s.starts_with("https://") => {
            Ok(Remote::Http(s.to_string()))
        }
        s if s.starts_with("ssh://") => Ok(Remote::Ssh(s.to_string())),
        _ => return Err("unsupported scheme"),
    }
}

#[derive(clap::Parser, Debug)]
#[command(name = "josh-proxy")]
pub struct Args {
    #[arg(long, required = true, value_parser = parse_remote)]
    pub remote: Vec<Remote>,
    #[arg(long, required = true)]
    pub local: Option<String>,
    #[arg(name = "poll", long)]
    pub poll_user: Option<String>,
    #[arg(long, help = "Run git gc during maintenance")]
    pub gc: bool,
    #[arg(long)]
    pub require_auth: bool,
    #[arg(long)]
    pub no_background: bool,

    #[arg(short, help = "DEPRECATED - no effect!")]
    _n: Option<String>,

    #[arg(long, default_value = "8000")]
    pub port: u16,
    #[arg(
        short,
        default_value = "0",
        help = "Duration between forced cache refresh"
    )]
    #[arg(long, short)]
    pub cache_duration: u64,
    #[arg(long, help = "Proxy static resource requests to a different URL")]
    pub static_resource_proxy_target: Option<String>,
    #[arg(long, help = "Filter to be prefixed to all queries of this instance")]
    pub filter_prefix: Option<String>,
}
