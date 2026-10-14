#[derive(Debug, clap::Parser)]
pub struct FetchArgs {
    /// Mount-derived remote identity to fetch; all remotes when omitted
    #[arg(short = 'r', long = "remote", add = crate::completion::remote())]
    pub remote: Option<String>,
}
