use crate::cli;
use josh_core::cache::{CacheStack, TransactionContext};
use josh_core::{JoshResult, josh_error};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

pub type FetchTimers = HashMap<String, std::time::Instant>;
pub type Polls = Arc<std::sync::Mutex<HashSet<(String, crate::auth::Handle, String)>>>;

pub type HeadsMap = Arc<RwLock<HashMap<String, String>>>;

#[derive(Serialize, Clone, Debug)]
pub enum JoshProxyUpstream {
    Http(String),
    Ssh(String),
    Both { http: String, ssh: String },
}

impl JoshProxyUpstream {
    pub fn get(&self, protocol: UpstreamProtocol) -> Option<String> {
        match (self, protocol) {
            (JoshProxyUpstream::Http(http), UpstreamProtocol::Http)
            | (JoshProxyUpstream::Both { http, .. }, UpstreamProtocol::Http) => Some(http.clone()),
            (JoshProxyUpstream::Ssh(ssh), UpstreamProtocol::Ssh)
            | (JoshProxyUpstream::Both { http: _, ssh }, UpstreamProtocol::Ssh) => {
                Some(ssh.clone())
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UpstreamProtocol {
    Http,
    Ssh,
}

#[derive(Clone)]
pub struct JoshProxyService {
    pub port: String,
    pub repo_path: std::path::PathBuf,
    pub upstream: JoshProxyUpstream,
    pub require_auth: bool,
    pub poll_user: Option<String>,
    pub cache_duration: u64,
    pub filter_prefix: Option<String>,
    pub cache: Arc<CacheStack>,
    pub fetch_timers: Arc<RwLock<FetchTimers>>,
    pub heads_map: HeadsMap,
    pub fetch_permits: Arc<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>>,
    pub filter_permits: Arc<tokio::sync::Semaphore>,
    pub poll: Polls,
}

impl JoshProxyService {
    pub fn open_overlay(
        &self,
        ref_prefix: Option<&str>,
    ) -> JoshResult<josh_core::cache::Transaction> {
        TransactionContext::new(self.repo_path.join("overlay"), self.cache.clone()).open(ref_prefix)
    }

    pub fn open_mirror(
        &self,
        ref_prefix: Option<&str>,
    ) -> JoshResult<josh_core::cache::Transaction> {
        TransactionContext::new(self.repo_path.join("mirror"), self.cache.clone()).open(ref_prefix)
    }
}

impl std::fmt::Debug for JoshProxyService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoshProxyService")
            .field("repo_path", &self.repo_path)
            .field("upstream", &self.upstream)
            .finish()
    }
}

/// Turn a list of [cli::Remote] into a [JoshProxyUpstream] struct.
pub fn make_upstream(remotes: &Vec<cli::Remote>) -> JoshResult<JoshProxyUpstream> {
    if remotes.is_empty() {
        unreachable!() // already checked in the parser
    } else if remotes.len() == 1 {
        Ok(match &remotes[0] {
            cli::Remote::Http(url) => JoshProxyUpstream::Http(url.to_string()),
            cli::Remote::Ssh(url) => JoshProxyUpstream::Ssh(url.to_string()),
        })
    } else if remotes.len() == 2 {
        Ok(match (&remotes[0], &remotes[1]) {
            (cli::Remote::Http(_), cli::Remote::Http(_))
            | (cli::Remote::Ssh(_), cli::Remote::Ssh(_)) => {
                return Err(josh_error("two cli::remotes of the same type passed"));
            }
            (cli::Remote::Http(http_url), cli::Remote::Ssh(ssh_url))
            | (cli::Remote::Ssh(ssh_url), cli::Remote::Http(http_url)) => JoshProxyUpstream::Both {
                http: http_url.to_string(),
                ssh: ssh_url.to_string(),
            },
        })
    } else {
        Err(josh_error("too many remotes"))
    }
}
