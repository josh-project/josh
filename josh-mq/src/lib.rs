use serde::{Deserialize, Serialize};

pub mod cli;
pub mod config;

#[derive(Serialize, Deserialize)]
pub struct Remote {
    pub url: String,
    pub main: String,
    pub credential: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub remotes: std::collections::BTreeMap<String, Remote>,
}
