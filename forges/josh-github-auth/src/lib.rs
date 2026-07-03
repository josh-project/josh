pub mod app_flow;
pub mod device_flow;
pub mod middleware;

pub const APP_CLIENT_ID: &str = "Ov23lijvAWwDiQDwZGhN";

/// Check if the given URL is a GitHub URL.
pub fn is_github_url(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h == "github.com" || h.ends_with(".github.com"))
        })
        .unwrap_or(false)
}
