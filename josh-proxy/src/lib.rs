pub mod auth;
pub mod juniper_hyper;

#[macro_use]
extern crate lazy_static;

fn baseref_and_options(refname: &str) -> josh::JoshResult<(String, String, Vec<String>)> {
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
    pub auth: auth::Handle,
    pub port: String,
    pub filter_spec: String,
    pub base_ns: String,
    pub git_ns: String,
    pub git_dir: String,
}

pub fn process_repo_update(repo_update: RepoUpdate) -> josh::JoshResult<String> {
    let mut resp = String::new();

    let p = std::path::PathBuf::from(&repo_update.git_dir)
        .join("refs/namespaces")
        .join(&repo_update.git_ns)
        .join("push_options");

    let push_options_string = std::fs::read_to_string(p)?;
    let push_options: std::collections::HashMap<String, String> =
        serde_json::from_str(&push_options_string)?;

    for (refname, (old, new)) in repo_update.refs.iter() {
        tracing::debug!("REPO_UPDATE env ok");

        let transaction = josh::cache::Transaction::open(
            &std::path::Path::new(&repo_update.git_dir),
            Some(&format!("refs/josh/upstream/{}/", repo_update.base_ns)),
        )?;

        let old = git2::Oid::from_str(old)?;

        let (baseref, push_to, options) = baseref_and_options(refname)?;
        let josh_merge = push_options.contains_key("merge");

        tracing::debug!("push options: {:?}", push_options);
        tracing::debug!("josh-merge: {:?}", josh_merge);

        let old = if old == git2::Oid::zero() {
            let rev = format!("refs/namespaces/{}/{}", repo_update.git_ns, &baseref);
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

        let original_target_ref = if let Some(base) = push_options.get("base") {
            transaction.refname(&base)
        } else {
            transaction.refname(&baseref)
        };

        let original_target =
            if let Ok(oid) = transaction.repo().refname_to_id(&original_target_ref) {
                tracing::debug!(
                    "push: original_target oid: {:?}, original_target_ref: {:?}",
                    oid,
                    original_target_ref
                );
                oid
            } else {
                return Err(josh::josh_error(&unindent::unindent(&format!(
                    r###"
                    Branch {:?} does not exist on remote.
                    If you want to create it, pass "-o base=<branchname>"
                    to specify a base branch.
                    "###,
                    baseref
                ))));
            };

        let reparent_orphans = if push_options.contains_key("create") {
            Some(original_target)
        } else {
            None
        };

        let amends = std::collections::HashMap::new();
        //let amends = {
        //    let gerrit_changes = format!(
        //        "refs/josh/upstream/{}/refs/changes/*",
        //        repo_update.base_ns,
        //    );
        //    let mut amends = std::collections::HashMap::new();
        //    for reference in
        //        transaction.repo().references_glob(&gerrit_changes)?
        //    {
        //        if let Ok(commit) = transaction.repo().find_commit(
        //            reference?.target().unwrap_or(git2::Oid::zero()),
        //        ) {
        //            if let Some(id) = josh::get_change_id(&commit) {
        //                amends.insert(id, commit.id());
        //            }
        //        }
        //    }
        //    amends
        //};

        let filterobj = josh::filter::parse(&repo_update.filter_spec)?;
        let new_oid = git2::Oid::from_str(&new)?;
        let backward_new_oid = {
            tracing::debug!("=== MORE");

            tracing::debug!("=== processed_old {:?}", old);

            match josh::history::unapply_filter(
                &transaction,
                filterobj,
                original_target,
                old,
                new_oid,
                josh_merge,
                reparent_orphans,
                &amends,
            )? {
                josh::UnapplyResult::Done(rewritten) => {
                    tracing::debug!("rewritten");
                    rewritten
                }
                josh::UnapplyResult::BranchDoesNotExist => {
                    return Err(josh::josh_error("branch does not exist on remote"));
                }
                josh::UnapplyResult::RejectMerge(parent_count) => {
                    return Err(josh::josh_error(&format!(
                        "rejecting merge with {} parents",
                        parent_count
                    )));
                }
                josh::UnapplyResult::RejectAmend(msg) => {
                    return Err(josh::josh_error(&format!(
                        "rejecting to amend {:?} with conflicting changes",
                        msg
                    )));
                }
            }
        };

        let oid_to_push = if josh_merge {
            let backward_commit = transaction.repo().find_commit(backward_new_oid)?;
            if let Ok(Ok(base_commit)) = transaction
                .repo()
                .revparse_single(&original_target_ref)
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
            format!("{}{}{}", push_to, "%", options.join(","))
        } else {
            push_to
        };

        let reapply = josh::filter::apply_to_commit(
            filterobj,
            &transaction.repo().find_commit(oid_to_push)?,
            &transaction,
        )?;

        let (text, status) = push_head_url(
            &transaction.repo(),
            oid_to_push,
            &push_with_options,
            &repo_update.remote_url,
            &repo_update.auth,
            &repo_update.git_ns,
        )?;

        let warnings = josh::filter::compute_warnings(
            &transaction,
            filterobj,
            transaction.repo().find_commit(oid_to_push)?.tree()?,
        );

        let mut warning_str = "".to_owned();
        if warnings.len() > 0 {
            let warnings = warnings.iter();

            warning_str += "\nwarnings:";
            for warn in warnings {
                warning_str += "\n";
                warning_str.push_str(&warn);
            }
        }

        resp = format!("{}{}{}", resp, text, warning_str);

        if new_oid != reapply {
            transaction.repo().reference(
                &format!(
                    "refs/josh/rewrites/{}/{:?}/r_{}",
                    repo_update.base_ns,
                    filterobj.id(),
                    reapply
                ),
                reapply,
                true,
                "reapply",
            )?;
            resp = format!("{}\nREWRITE({} -> {})", resp, new_oid, reapply);
            tracing::debug!("REWRITE({} -> {})", new_oid, reapply);
        }

        if status == 0 {
            return Ok(resp);
        }
        return Err(josh::josh_error(&resp));
    }

    return Ok("".to_string());
}

fn push_head_url(
    repo: &git2::Repository,
    oid: git2::Oid,
    refname: &str,
    url: &str,
    auth: &auth::Handle,
    namespace: &str,
) -> josh::JoshResult<(String, i32)> {
    let rn = format!("refs/{}", &namespace);

    let spec = format!("{}:{}", &rn, &refname);

    let shell = josh::shell::Shell {
        cwd: repo.path().to_owned(),
    };
    let (username, password) = auth.parse()?;
    let nurl = url_with_auth(&url, &username);
    let cmd = format!("git push {} '{}'", &nurl, &spec);
    let mut fakehead = repo.reference(&rn, oid, true, "push_head_url")?;
    let (stdout, stderr, status) = shell.command_env(&cmd, &[], &[("GIT_PASSWORD", &password)]);
    fakehead.delete()?;
    tracing::debug!("{}", &stderr);
    tracing::debug!("{}", &stdout);

    let stderr = stderr.replace(&rn, "JOSH_PUSH");

    return Ok((stderr, status));
}

pub fn create_repo(path: &std::path::Path) -> josh::JoshResult<()> {
    tracing::debug!("init base repo: {:?}", path);
    std::fs::create_dir_all(path).expect("can't create_dir_all");
    git2::Repository::init_bare(path)?;
    let shell = josh::shell::Shell {
        cwd: path.to_path_buf(),
    };
    shell.command("git config http.receivepack true");
    shell.command("git config uploadpack.allowsidebandall true");
    shell.command("git config user.name josh");
    shell.command("git config user.email josh@localhost");
    shell.command("git config uploadpack.allowAnySHA1InWant true");
    shell.command("git config uploadpack.allowReachableSHA1InWant true");
    shell.command("git config uploadpack.allowTipSha1InWant true");
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
    shell.command(&"git pack-refs --all");

    if std::env::var_os("JOSH_KEEP_NS") == None {
        std::fs::remove_dir_all(path.join("refs/namespaces")).ok();
    }
    tracing::info!("repo initialized");
    return Ok(());
}

fn url_with_auth(url: &str, username: &str) -> String {
    if username != "" {
        let splitted: Vec<&str> = url.splitn(2, "://").collect();
        let proto = splitted[0];
        let rest = splitted[1];
        let username =
            percent_encoding::utf8_percent_encode(&username, percent_encoding::NON_ALPHANUMERIC)
                .to_string();
        format!("{}://{}@{}", &proto, &username, &rest)
    } else {
        let splitted: Vec<&str> = url.splitn(2, "://").collect();
        let proto = splitted[0];
        let rest = splitted[1];
        format!("{}://{}@{}", &proto, "annonymous", &rest)
    }
}

pub fn fetch_refs_from_url(
    path: &std::path::Path,
    upstream_repo: &str,
    url: &str,
    refs_prefixes: &[String],
    auth: &auth::Handle,
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
    let (username, password) = auth.parse()?;
    let nurl = url_with_auth(&url, &username);

    let cmd = format!("git fetch --prune --no-tags {} {}", &nurl, &specs.join(" "));
    tracing::info!("fetch_refs_from_url {:?} {:?} {:?}", cmd, path, "");

    let (_stdout, stderr, _) = shell.command_env(&cmd, &[], &[("GIT_PASSWORD", &password)]);
    tracing::debug!("fetch_refs_from_url done {:?} {:?} {:?}", cmd, path, stderr);
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

pub struct TmpGitNamespace {
    name: String,
    repo_path: std::path::PathBuf,
    _span: tracing::Span,
}

impl TmpGitNamespace {
    pub fn new(repo_path: &std::path::Path, span: tracing::Span) -> TmpGitNamespace {
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
        let request_tmp_namespace = self.repo_path.join("refs/namespaces").join(&self.name);
        std::fs::remove_dir_all(&request_tmp_namespace).unwrap_or_else(|e| {
            tracing::error!(
                "remove_dir_all {:?} failed, error:{:?}",
                request_tmp_namespace,
                e
            )
        });
    }
}
