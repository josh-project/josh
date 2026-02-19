pub mod device_flow;
pub mod middleware;
pub mod token;

pub const APP_CLIENT_ID: &str = "Iv23liK2qIIUHy5iILiz";
pub const APP_ID: &str = "2871336";

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
