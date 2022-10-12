use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct ServeNamespace {
    pub stdin_pipe: PathBuf,
    pub stdout_pipe: PathBuf,
    pub ssh_socket: PathBuf,
    pub query: String,
}
