use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum RequestedCommand {
    GitUploadPack,
    GitUploadArchive,
    GitReceivePack,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ServeNamespace {
    pub stdin_pipe: PathBuf,
    pub stdout_pipe: PathBuf,
    pub ssh_socket: PathBuf,
    pub query: String,
}
