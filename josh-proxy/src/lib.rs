extern crate futures;
extern crate hyper;
extern crate regex;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_process;
extern crate tracing;

#[macro_use]
extern crate lazy_static;

use self::futures::future::Future;
use self::futures::Stream;
use self::hyper::header::ContentEncoding;
use self::hyper::header::ContentLength;
use self::hyper::header::ContentType;
use self::hyper::server::{Request, Response};
use self::tokio_process::CommandExt;
use self::tracing::{debug, span, trace, Level};
use git2::Oid;
use std::collections::HashMap;
use std::env::current_exe;
use std::fs;
use std::io;
use std::io::BufRead;
use std::io::Read;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use tracing::event;

pub mod gerrit;

pub type BoxedFuture<T> = Box<dyn Future<Item = T, Error = hyper::Error>>;

pub fn do_cgi(
    req: Request,
    cmd: Command,
    handle: tokio_core::reactor::Handle,
) -> Box<dyn Future<Item = Response, Error = hyper::Error>> {
    span!(Level::TRACE, "do_cgi");
    let mut cmd = cmd;
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::inherit());
    cmd.stdin(Stdio::piped());
    cmd.env("SERVER_SOFTWARE", "hyper")
        .env("SERVER_NAME", "localhost") // TODO
        .env("GATEWAY_INTERFACE", "CGI/1.1")
        .env("SERVER_PROTOCOL", "HTTP/1.1") // TODO
        .env("SERVER_PORT", "80") // TODO
        .env("REQUEST_METHOD", format!("{}", req.method()))
        .env("SCRIPT_NAME", "") // TODO
        .env("QUERY_STRING", req.query().unwrap_or(""))
        .env("REMOTE_ADDR", "") // TODO
        .env("AUTH_TYPE", "") // TODO
        .env("REMOTE_USER", "") // TODO
        .env(
            "CONTENT_TYPE",
            &format!(
                "{}",
                req.headers().get().unwrap_or(&ContentType::plaintext())
            ),
        )
        .env(
            "HTTP_CONTENT_ENCODING",
            &format!(
                "{}",
                req.headers().get().unwrap_or(&ContentEncoding(vec![]))
            ),
        )
        .env(
            "CONTENT_LENGTH",
            &format!("{}", req.headers().get().unwrap_or(&ContentLength(0))),
        );

    let mut child = cmd
        .spawn_async_with_handle(&handle.new_tokio_handle())
        .expect("can't spawn CGI command");

    let r = req.body().concat2().and_then(move |body| {
        tokio_io::io::write_all(child.stdin().take().unwrap(), body)
            .and_then(move |_| {
                child
                    .wait_with_output()
                    .map(build_response)
                    .map_err(|e| e.into())
            })
            .map_err(|e| e.into())
    });

    Box::new(r)
}

fn build_response(command_result: std::process::Output) -> Response {
    let _trace_s = span!(Level::TRACE, "build_response");
    let mut stdout = io::BufReader::new(command_result.stdout.as_slice());
    let mut stderr = io::BufReader::new(command_result.stderr.as_slice());

    let mut response = Response::new();

    let mut headers = vec![];
    for line in stdout.by_ref().lines() {
        event!(parent: &_trace_s, Level::TRACE, "STDOUT {:?}", line);
        if line.as_ref().unwrap().is_empty() {
            break;
        }
        let l: Vec<&str> =
            line.as_ref().unwrap().as_str().splitn(2, ": ").collect();
        for x in &l {
            headers.push(x.to_string());
        }
        if l[0] == "Status" {
            response.set_status(hyper::StatusCode::Unregistered(
                u16::from_str(l[1].split(" ").next().unwrap()).unwrap(),
            ));
        } else {
            response
                .headers_mut()
                .set_raw(l[0].to_string(), l[1].to_string());
        }
    }

    let mut data = vec![];
    stdout
        .read_to_end(&mut data)
        .expect("can't read command output");

    let mut stderrdata = vec![];
    stderr
        .read_to_end(&mut stderrdata)
        .expect("can't read command output");

    let err = String::from_utf8_lossy(&stderrdata);

    trace!("build_response err {:?}", &err);

    event!(parent: &_trace_s, Level::TRACE, ?err, ?headers);
    response.set_body(hyper::Chunk::from(data));

    response
}

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

pub fn process_repo_update(
    repo_update: HashMap<String, String>,
    _forward_maps: Arc<RwLock<josh::view_maps::ViewMaps>>,
    backward_maps: Arc<RwLock<josh::view_maps::ViewMaps>>,
) -> Result<String, josh::JoshError> {
    let ru = {
        let mut ru = repo_update.clone();
        ru.insert("password".to_owned(), "...".to_owned());
    };
    let _trace_s = span!(Level::TRACE, "process_repo_update", repo_update= ?ru);
    let refname = repo_update.get("refname").ok_or(josh::josh_error(""))?;
    let filter_spec =
        repo_update.get("filter_spec").ok_or(josh::josh_error(""))?;
    let old = repo_update.get("old").ok_or(josh::josh_error(""))?;
    let new = repo_update.get("new").ok_or(josh::josh_error(""))?;
    let username = repo_update.get("username").ok_or(josh::josh_error(""))?;
    let password = repo_update.get("password").ok_or(josh::josh_error(""))?;
    let remote_url =
        repo_update.get("remote_url").ok_or(josh::josh_error(""))?;
    let base_ns = repo_update.get("base_ns").ok_or(josh::josh_error(""))?;
    let git_dir = repo_update.get("GIT_DIR").ok_or(josh::josh_error(""))?;
    let git_ns = repo_update
        .get("GIT_NAMESPACE")
        .ok_or(josh::josh_error(""))?;
    debug!("REPO_UPDATE env ok");

    let repo = git2::Repository::init_bare(&Path::new(&git_dir))?;

    let old = Oid::from_str(old)?;

    let (baseref, push_to, options) = baseref_and_options(refname)?;
    let josh_merge = options.contains(&"josh-merge".to_string());

    debug!("push options: {:?}", options);
    debug!("XXX josh-merge: {:?}", josh_merge);

    let old = if old == Oid::zero() {
        let rev = format!("refs/namespaces/{}/{}", git_ns, &baseref);
        let oid = if let Ok(x) = repo.revparse_single(&rev) {
            x.id()
        } else {
            old
        };
        trace!("push: old oid: {:?}, rev: {:?}", oid, rev);
        oid
    } else {
        trace!("push: old oid: {:?}, refname: {:?}", old, refname);
        old
    };

    let viewobj = josh::filters::parse(&filter_spec);
    let new_oid = Oid::from_str(&new)?;
    let backward_new_oid = {
        debug!("=== MORE");

        debug!("=== processed_old {:?}", old);

        match josh::scratch::unapply_view(
            &repo,
            backward_maps,
            &*viewobj,
            old,
            new_oid,
        )? {
            josh::UnapplyView::Done(rewritten) => {
                debug!("rewritten");
                rewritten
            }
            josh::UnapplyView::BranchDoesNotExist => {
                return Err(josh::josh_error(
                    "branch does not exist on remote",
                ));
            }
            josh::UnapplyView::RejectMerge(parent_count) => {
                return Err(josh::josh_error(&format!(
                    "rejecting merge with {} parents",
                    parent_count
                )));
            }
        }
    };

    let oid_to_push = if josh_merge {
        let rev = format!("refs/namespaces/{}/{}", &base_ns, &baseref);
        let backward_commit = repo.find_commit(backward_new_oid)?;
        if let Ok(Ok(base_commit)) =
            repo.revparse_single(&rev).map(|x| x.peel_to_commit())
        {
            let merged_tree = repo
                .merge_commits(&base_commit, &backward_commit, None)?
                .write_tree_to(&repo)?;
            repo.commit(
                None,
                &backward_commit.author(),
                &backward_commit.committer(),
                &format!("Merge from {}", &filter_spec),
                &repo.find_tree(merged_tree)?,
                &[&base_commit, &backward_commit],
            )?
        } else {
            return Err(josh::josh_error("josh_merge failed"));
        }
    } else {
        backward_new_oid
    };

    let mut options = options;
    options.retain(|x| !x.starts_with("josh-"));
    let options = options;

    let push_with_options = if options.len() != 0 {
        push_to + "%" + &options.join(",")
    } else {
        push_to
    };

    return push_head_url(
        &repo,
        oid_to_push,
        &push_with_options,
        &remote_url,
        &username,
        &password,
        &git_ns,
    );
}

fn push_head_url(
    repo: &git2::Repository,
    oid: git2::Oid,
    refname: &str,
    url: &str,
    username: &str,
    password: &str,
    namespace: &str,
) -> josh::JoshResult<String> {
    let rn = format!("refs/{}", &namespace);

    let spec = format!("{}:{}", &rn, &refname);

    let shell = josh::shell::Shell {
        cwd: repo.path().to_owned(),
    };
    let nurl = {
        let splitted: Vec<&str> = url.splitn(2, "://").collect();
        let proto = splitted[0];
        let rest = splitted[1];
        format!("{}://{}@{}", &proto, &username, &rest)
    };
    let cmd = format!("git push {} '{}'", &nurl, &spec);
    let mut fakehead = repo.reference(&rn, oid, true, "push_head_url")?;
    let (stdout, stderr) =
        shell.command_env(&cmd, &[("GIT_PASSWORD", &password)]);
    fakehead.delete()?;
    debug!("{}", &stderr);
    debug!("{}", &stdout);

    let stderr = stderr.replace(&rn, "JOSH_PUSH");

    return Ok(stderr);
}

pub fn create_repo(path: &Path) -> josh::JoshResult<()> {
    debug!("init base repo: {:?}", path);
    fs::create_dir_all(path).expect("can't create_dir_all");
    git2::Repository::init_bare(path)?;
    if !path.join("hooks/update").exists() {
        let shell = josh::shell::Shell {
            cwd: path.to_path_buf(),
        };
        shell.command("git config http.receivepack true");
        let ce = current_exe().expect("can't find path to exe");
        shell.command("rm -Rf hooks");
        shell.command("mkdir hooks");
        symlink(ce, path.join("hooks").join("update"))
            .expect("can't symlink update hook");
        shell.command(&format!(
            "git config credential.helper '!f() {{ echo \"password=\"$GIT_PASSWORD\"\"; }}; f'"
        ));
        shell.command(&"git config gc.auto 0");
    }
    tracing::info!("repo initialized");
    return Ok(());
}

pub fn fetch_refs_from_url(
    path: &Path,
    upstream_repo: &str,
    url: &str,
    refs_prefixes: &[&str],
    username: &str,
    password: &str,
) -> Result<(), git2::Error> {
    for refs_prefix in refs_prefixes {
        let spec = format!(
            "+{}:refs/namespaces/{}/{}",
            &refs_prefix,
            josh::to_ns(upstream_repo),
            &refs_prefix
        );

        let shell = josh::shell::Shell {
            cwd: path.to_owned(),
        };
        let nurl = {
            let splitted: Vec<&str> = url.splitn(2, "://").collect();
            let proto = splitted[0];
            let rest = splitted[1];
            format!("{}://{}@{}", &proto, &username, &rest)
        };

        let cmd = format!("git fetch {} '{}'", &nurl, &spec);
        tracing::info!("fetch_refs_from_url {:?} {:?} {:?}", cmd, path, "");

        let (_stdout, stderr) =
            shell.command_env(&cmd, &[("GIT_PASSWORD", &password)]);
        debug!("fetch_refs_from_url done {:?} {:?} {:?}", cmd, path, stderr);
        if stderr.contains("fatal: Authentication failed") {
            return Err(git2::Error::from_str("auth"));
        }
        if stderr.contains("fatal:") {
            return Err(git2::Error::from_str("error"));
        }
        if stderr.contains("error:") {
            return Err(git2::Error::from_str("error"));
        }
    }
    return Ok(());
}

pub fn body2string(body: hyper::Chunk) -> String {
    let mut buffer: Vec<u8> = Vec::new();
    for i in body {
        buffer.push(i);
    }

    String::from_utf8(buffer).unwrap_or("".to_string())
}

pub struct TmpGitNamespace {
    pub name: String,
    repo_path: std::path::PathBuf,
}

impl TmpGitNamespace {
    pub fn new(repo_path: &std::path::Path) -> TmpGitNamespace {
        TmpGitNamespace {
            name: format!("request_{}", uuid::Uuid::new_v4()),
            repo_path: repo_path.to_owned(),
        }
    }
}

impl Drop for TmpGitNamespace {
    fn drop(&mut self) {
        let request_tmp_namespace =
            self.repo_path.join("refs/namespaces").join(&self.name);
        std::fs::remove_dir_all(request_tmp_namespace).unwrap_or_else(|e| {
            tracing::warn!("remove_dir_all failed: {:?}", e)
        });
    }
}

