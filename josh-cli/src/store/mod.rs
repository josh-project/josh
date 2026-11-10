use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use gix::bstr::ByteSlice;

use anyhow::{Context, anyhow, bail};
use josh_changes::{JoshRef, SUPER_REF};
use serde::{Deserialize, Serialize};

use crate::config::{RemoteConfig, StoredRemote, read_legacy_remote_config};
use crate::context::{CachePolicy, ExecutionContext};
use crate::forge::{Forge, GerritMode};

mod config;
mod git;
mod migrate;
mod projection;
mod remote;
mod worktree;

pub use config::*;
pub use migrate::*;
pub use projection::*;
pub use remote::*;
pub use worktree::*;

use config::{
    ROOT_PROJECTION, STORE_VERSION, SUPER_CONFIG_PATH, SuperConfig, SuperWorktree, parse_mount_path,
};
use git::*;
use projection::{refs_in_superhistory, update_super_ref};
use worktree::{configure_worktree_remote, fetch_worktrees};
