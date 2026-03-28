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
    pub stdin_sock: PathBuf,
    pub stdout_sock: PathBuf,
    pub ssh_socket: PathBuf,
    pub query: String,
}
