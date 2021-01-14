fn baseref_and_options(
    refname: &str,
) -> josh::JoshResult<(String, String, Vec<String>)> {
    let mut split = refname.splitn(2, '%');
    let push_to = split.next().ok_or(josh::josh_error("no next"))?.to_owned();

    let options = if let Some(options) = split.next() {
        options.split(',').map(|x| x.to_string()).collect()
    } else {
        vec![]
    };

    let mut baseref = push_to.to_owned();

    if baseref.starts_with("refs/for") {
        baseref = baseref.replacen("refs/for", "refs/heads", 1)
    }
    if baseref.starts_with("refs/drafts") {
        baseref = baseref.replacen("refs/drafts", "refs/heads", 1)
    }
    return Ok((baseref, push_to, options));
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct RepoUpdate {
    pub refs: std::collections::HashMap<String, (String, String)>,
    pub remote_url: String,
    pub username: String,
    pub password: HashedPassword,
    pub port: String,
    pub filter_spec: String,
    pub base_ns: String,
    pub git_ns: String,
    pub git_dir: String,
}

#[tracing::instrument(skip(credential_store))]
pub fn process_repo_update(
    credential_store: std::sync::Arc<std::sync::RwLock<CredentialStore>>,
    repo_update: RepoUpdate,
) -> josh::JoshResult<String> {
    let mut resp = String::new();

    let p = std::path::PathBuf::from(&repo_update.git_dir)
        .join("refs/namespaces")
        .join(&repo_update.git_ns)
        .join("push_options");

    let push_options_string = std::fs::read_to_string(p)?;
    let push_options: Vec<&str> = push_options_string.split("\n").collect();

    for (refname, (old, new)) in repo_update.refs.iter() {
        tracing::debug!("REPO_UPDATE env ok");

        let transaction = josh::filter_cache::Transaction::open(
            &std::path::Path::new(&repo_update.git_dir),
        )?;

        let old = git2::Oid::from_str(old)?;

        let (baseref, push_to, options) = baseref_and_options(refname)?;
        let josh_merge = push_options.contains(&"merge");

        tracing::debug!("push options: {:?}", push_options);
        tracing::debug!("josh-merge: {:?}", josh_merge);

        let old = if old == git2::Oid::zero() {
            let rev =
                format!("refs/namespaces/{}/{}", repo_update.git_ns, &baseref);
            let oid = if let Ok(x) = transaction.repo().revparse_single(&rev) {
                x.id()
            } else {
                old
            };
            tracing::debug!("push: old oid: {:?}, rev: {:?}", oid, rev);
            oid
        } else {
            tracing::debug!("push: old oid: {:?}, refname: {:?}", old, refname);
            old
        };

        let unfiltered_old = {
            let rev = format!(
                "refs/josh/upstream/{}/{}",
                repo_update.base_ns, &baseref
            );
            let oid = transaction
                .repo()
                .refname_to_id(&rev)
                .unwrap_or(git2::Oid::zero());
            tracing::debug!(
                "push: unfiltered_old oid: {:?}, rev: {:?}",
                oid,
                rev
            );
            oid
        };

        let amends = {
            let gerrit_changes = format!(
                "refs/josh/upstream/{}/refs/gerrit_changes/all",
                repo_update.base_ns,
            );
            let mut amends = std::collections::HashMap::new();
            if let Ok(tree) = transaction
                .repo()
                .find_reference(&gerrit_changes)
                .and_then(|x| x.peel_to_commit())
                .and_then(|x| x.tree())
            {
                tree.walk(git2::TreeWalkMode::PreOrder, |_, entry| {
                    if let Ok(commit) =
                        transaction.repo().find_commit(entry.id())
                    {
                        if let Some(id) = josh::get_change_id(&commit) {
                            amends.insert(id, commit.id());
                        }
                    }
                    git2::TreeWalkResult::Ok
                });
            }
            amends
        };

        let filterobj = josh::filters::parse(&repo_update.filter_spec)?;
        let new_oid = git2::Oid::from_str(&new)?;
        let backward_new_oid = {
            tracing::debug!("=== MORE");

            tracing::debug!("=== processed_old {:?}", old);

            match josh::history::unapply_filter(
                &transaction,
                filterobj,
                unfiltered_old,
                old,
                new_oid,
                josh_merge,
                &amends,
            )? {
                josh::UnapplyFilter::Done(rewritten) => {
                    tracing::debug!("rewritten");
                    rewritten
                }
                josh::UnapplyFilter::BranchDoesNotExist => {
                    return Err(josh::josh_error(
                        "branch does not exist on remote",
                    ));
                }
                josh::UnapplyFilter::RejectMerge(parent_count) => {
                    return Err(josh::josh_error(&format!(
                        "rejecting merge with {} parents",
                        parent_count
                    )));
                }
                josh::UnapplyFilter::RejectAmend(msg) => {
                    return Err(josh::josh_error(&format!(
                        "rejecting to amend {:?} with conflicting changes",
                        msg
                    )));
                }
            }
        };

        let oid_to_push = if josh_merge {
            let rev = format!(
                "refs/josh/upstream/{}/{}",
                &repo_update.base_ns, &baseref
            );
            let backward_commit =
                transaction.repo().find_commit(backward_new_oid)?;
            if let Ok(Ok(base_commit)) = transaction
                .repo()
                .revparse_single(&rev)
                .map(|x| x.peel_to_commit())
            {
                let merged_tree = transaction
                    .repo()
                    .merge_commits(&base_commit, &backward_commit, None)?
                    .write_tree_to(&transaction.repo())?;
                transaction.repo().commit(
                    None,
                    &backward_commit.author(),
                    &backward_commit.committer(),
                    &format!("Merge from {}", &repo_update.filter_spec),
                    &transaction.repo().find_tree(merged_tree)?,
                    &[&base_commit, &backward_commit],
                )?
            } else {
                return Err(josh::josh_error("josh_merge failed"));
            }
        } else {
            backward_new_oid
        };

        let push_with_options = if options.len() != 0 {
            push_to + "%" + &options.join(",")
        } else {
            push_to
        };

        let password = credential_store
            .read()?
            .get(&repo_update.password)
            .unwrap_or(&Password {
                value: "".to_owned(),
            })
            .to_owned();

        let reapply = josh::filters::apply_to_commit(
            filterobj,
            &transaction.repo().find_commit(oid_to_push)?,
            &transaction,
        )?;

        resp = format!(
            "{}{}",
            resp,
            push_head_url(
                &transaction.repo(),
                oid_to_push,
                &push_with_options,
                &repo_update.remote_url,
                &repo_update.username,
                &password,
                &repo_update.git_ns,
            )?
        );

        if new_oid != reapply {
            transaction.repo().reference(
                &format!(
                    "refs/josh/rewrites/{}/r_{}",
                    repo_update.base_ns, reapply
                ),
                reapply,
                true,
                "reapply",
            )?;
            resp = format!("{}\nREWRITE({} -> {})", resp, new_oid, reapply);
        }
    }

    return Ok(resp);
}

fn push_head_url(
    repo: &git2::Repository,
    oid: git2::Oid,
    refname: &str,
    url: &str,
    username: &str,
    password: &Password,
    namespace: &str,
) -> josh::JoshResult<String> {
    let rn = format!("refs/{}", &namespace);

    let spec = format!("{}:{}", &rn, &refname);

    let shell = josh::shell::Shell {
        cwd: repo.path().to_owned(),
    };
    let nurl = if username != "" {
        let splitted: Vec<&str> = url.splitn(2, "://").collect();
        let proto = splitted[0];
        let rest = splitted[1];
        format!("{}://{}@{}", &proto, &username, &rest)
    } else {
        url.to_owned()
    };
    let cmd = format!("git push {} '{}'", &nurl, &spec);
    let mut fakehead = repo.reference(&rn, oid, true, "push_head_url")?;
    let (stdout, stderr, status) =
        shell.command_env(&cmd, &[], &[("GIT_PASSWORD", &password.value)]);
    fakehead.delete()?;
    tracing::debug!("{}", &stderr);
    tracing::debug!("{}", &stdout);

    let stderr = stderr.replace(&rn, "JOSH_PUSH");

    if status != 0 {
        return Err(josh::josh_error(&stderr));
    }

    return Ok(stderr);
}

pub fn create_repo(path: &std::path::Path) -> josh::JoshResult<()> {
    tracing::debug!("init base repo: {:?}", path);
    std::fs::create_dir_all(path).expect("can't create_dir_all");
    git2::Repository::init_bare(path)?;
    let shell = josh::shell::Shell {
        cwd: path.to_path_buf(),
    };
    shell.command("git config http.receivepack true");
    shell.command("git config receive.advertisePushOptions true");
    let ce = std::env::current_exe().expect("can't find path to exe");
    shell.command("rm -Rf hooks");
    shell.command("mkdir hooks");
    std::os::unix::fs::symlink(ce.clone(), path.join("hooks").join("update"))
        .expect("can't symlink update hook");
    std::os::unix::fs::symlink(ce, path.join("hooks").join("pre-receive"))
        .expect("can't symlink pre-receive hook");
    shell.command(&format!(
        "git config credential.helper '!f() {{ echo \"password=\"$GIT_PASSWORD\"\"; }}; f'"
    ));
    shell.command(&"git config gc.auto 0");

    if std::env::var_os("JOSH_KEEP_NS") == None {
        std::fs::remove_dir_all(path.join("refs/namespaces")).ok();
    }
    tracing::info!("repo initialized");
    return Ok(());
}

#[tracing::instrument(skip(credential_store))]
pub fn fetch_refs_from_url(
    path: &std::path::Path,
    upstream_repo: &str,
    url: &str,
    refs_prefixes: &[String],
    username: &str,
    password: &HashedPassword,
    credential_store: std::sync::Arc<std::sync::RwLock<CredentialStore>>,
) -> josh::JoshResult<bool> {
    let specs: Vec<_> = refs_prefixes
        .iter()
        .map(|r| {
            format!(
                "'+{}:refs/josh/upstream/{}/{}'",
                &r,
                josh::to_ns(upstream_repo),
                &r
            )
        })
        .collect();

    let shell = josh::shell::Shell {
        cwd: path.to_owned(),
    };
    let nurl = if username != "" {
        let splitted: Vec<&str> = url.splitn(2, "://").collect();
        let proto = splitted[0];
        let rest = splitted[1];
        format!("{}://{}@{}", &proto, &username, &rest)
    } else {
        let splitted: Vec<&str> = url.splitn(2, "://").collect();
        let proto = splitted[0];
        let rest = splitted[1];
        format!("{}://{}@{}", &proto, "annonymous", &rest)
    };

    let cmd = format!("git fetch --no-tags {} {}", &nurl, &specs.join(" "));
    tracing::info!("fetch_refs_from_url {:?} {:?} {:?}", cmd, path, "");

    let password = credential_store
        .read()?
        .get(&password)
        .unwrap_or(&Password {
            value: "".to_owned(),
        })
        .to_owned();

    let (_stdout, stderr, _) =
        shell.command_env(&cmd, &[], &[("GIT_PASSWORD", &password.value)]);
    tracing::debug!(
        "fetch_refs_from_url done {:?} {:?} {:?}",
        cmd,
        path,
        stderr
    );
    if stderr.contains("fatal: Authentication failed") {
        return Ok(false);
    }
    if stderr.contains("fatal:") {
        return Err(josh::josh_error(&format!("git error: {:?}", stderr)));
    }
    if stderr.contains("error:") {
        return Err(josh::josh_error(&format!("git error: {:?}", stderr)));
    }
    return Ok(true);
}

// Wrapper struct for storing passwords to avoid having
// them output to traces by accident
#[derive(Clone)]
pub struct Password {
    pub value: String,
}

#[derive(Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HashedPassword {
    pub hash: String,
}

pub type CredentialStore = std::collections::HashMap<HashedPassword, Password>;

impl std::fmt::Debug for HashedPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HashedPassword")
            .field("value", &self.hash)
            .finish()
    }
}

pub struct TmpGitNamespace {
    name: String,
    repo_path: std::path::PathBuf,
    _span: tracing::Span,
}

impl TmpGitNamespace {
    pub fn new(
        repo_path: &std::path::Path,
        span: tracing::Span,
    ) -> TmpGitNamespace {
        let n = format!("request_{}", uuid::Uuid::new_v4());
        let n2 = n.clone();
        TmpGitNamespace {
            name: n,
            repo_path: repo_path.to_owned(),
            _span: tracing::span!(
                parent: span,
                tracing::Level::TRACE,
                "TmpGitNamespace",
                name = n2.as_str(),
            ),
        }
    }

    pub fn name(&self) -> &str {
        return &self.name;
    }
    pub fn reference(&self, refname: &str) -> String {
        return format!("refs/namespaces/{}/{}", &self.name, refname);
    }
}

impl std::fmt::Debug for TmpGitNamespace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JoshProxyService")
            .field("repo_path", &self.repo_path)
            .field("name", &self.name)
            .finish()
    }
}

impl Drop for TmpGitNamespace {
    fn drop(&mut self) {
        if std::env::var_os("JOSH_KEEP_NS") != None {
            return;
        }
        let request_tmp_namespace =
            self.repo_path.join("refs/namespaces").join(&self.name);
        std::fs::remove_dir_all(&request_tmp_namespace).unwrap_or_else(|e| {
            tracing::error!(
                "remove_dir_all {:?} failed, error:{:?}",
                request_tmp_namespace,
                e
            )
        });
    }
}
