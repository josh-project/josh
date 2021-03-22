use super::*;

#[derive(Switch, Clone)]
pub enum AppRoute {
    #[to = "/~/browse/{*}@{*}[{*}]/{*}"]
    Browse(String, String, String, String),
}
