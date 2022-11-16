use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum RequestedCommand {
    GitUploadPack,
    GitUploadArchive,
    GitReceivePack,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ServeNamespace {
    pub command: RequestedCommand,
    pub stdin_pipe: PathBuf,
    pub stdout_pipe: PathBuf,
    pub ssh_socket: PathBuf,
    pub query: String,
}
