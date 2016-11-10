extern crate centralgithook;
extern crate fern;
extern crate git2;
extern crate regex;
extern crate tempdir;

#[macro_use]
extern crate log;

use centralgithook::*;
use git2::Oid;
use regex::Regex;
use std::env::current_exe;
use std::env;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::process::exit;
use tempdir::TempDir;

fn main()
{
    let logfilename = Path::new("/tmp/centralgit.log");
    let logger_config = fern::DispatchConfig {
        format: Box::new(|msg: &str, level: &log::LogLevel, _location: &log::LogLocation| {
            format!("[{}] {}", level, msg)
        }),
        output: vec![fern::OutputConfig::file(logfilename)],
        level: log::LogLevelFilter::Trace,
    };
    fern::init_global_logger(logger_config, log::LogLevelFilter::Trace).expect("can't init logger");

    exit(main_ret());
}

fn setup_tmp_repo(td: &Path, scratch_dir: &Path, view: Option<&str>)
{
    let root = match view {
        Some(view) => view_ref_root(&view),
        None => "refs".to_string(),
    };

    debug!("setup_tmp_repo, root: {:?}", &root);
    let shell = Shell { cwd: td.to_path_buf() };

    let ce = current_exe().expect("can't find path to exe");
    shell.command("mkdir hooks");
    symlink(ce, td.join("hooks").join("update")).expect("can't symlink update hook");

    shell.command(&format!("cp {:?} {:?}", scratch_dir.join("HEAD"), td));
    symlink(scratch_dir.join(root), td.join("refs")).expect("can't symlink refs");
    symlink(scratch_dir.join("objects"), td.join("objects")).expect("can't symlink objects");

    shell.command(&format!("printf {} > view",
                           match view {
                               Some(view) => view,
                               None => ".",
                           }));

    shell.command(&format!("printf {} > orig", scratch_dir.to_string_lossy()));
}

fn cg_command(subcommand: &str) -> i32
{
    debug!("Command cg {:?}", &subcommand);
    if subcommand == "status" {
        println!("centralgit OK");
        return 0;
    }
    if subcommand == "log" {

        let _ = if let Ok(status) = Command::new("cat")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .arg("/tmp/centralgit.log")
            .status() {
            debug!("call ok");
            return status.code().unwrap_or(1);
        }
        else {
            debug!("failed to call cat /tmp/centralgit.log");
            return 1;
        };
    }
    return 0;
}

fn make_argvec(args: &str) -> Vec<String>
{
    let mut argvec = vec![];
    for c in args.split_whitespace() {
        argvec.push(format!("{}", c).trim_matches('\'').to_string());
    }
    return argvec;
}

fn call_git(command: &str, td: &Path, args: &str) -> i32
{
    let re_repo = Regex::new(r"(?P<repo>/.*[.]git\S*)").expect("can't compile regex");
    let rewritten_args = re_repo.replace_all(args, format!("{:?}", td).trim_matches('"'));

    return if let Ok(status) = Command::new(command)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .args(&make_argvec(&rewritten_args))
        .status() {
        debug!("call ok");
        status.code().unwrap_or(1)
    }
    else {
        debug!("failed to call git");
        1
    };
}

fn git_command(command: &str, args: &str) -> i32
{
    let td = TempDir::new("centralgit").expect("failed to create tempdir");

    let repo_name = {
        let re = Regex::new(r".*'/(?P<repo>.*[.]git).*'").expect("can't compile regex");
        let caps = re.captures(&args).expect("can't find repo name");
        caps.name("repo").unwrap()
    };

    let scratch_dir = Path::new("/tmp").join("centralgit_central").join(repo_name);
    let scratch = Scratch::new(&scratch_dir);

    let re = Regex::new(r".*'.*[.]git/(?P<view>\S+)'").expect("can't compile regex");
    if let Some(caps) = re.captures(&args) {
        let view = caps.name("view").unwrap();

        for branch in scratch.repo.branches(None).expect("could not get branches") {
            if let Ok((branch, _)) = branch {
                let branchname = branch.name().unwrap().unwrap().to_string();
                let r = branch.into_reference().target().expect("no ref");
                let viewobj = SubdirView::new(&Path::new(&view));
                let view_commit = scratch.apply_view(&viewobj, r).expect("can't apply view");
                scratch.repo
                    .reference(&view_ref(&view, &branchname),
                               view_commit,
                               true,
                               "apply_view")
                    .expect("can't create reference");
            };
        }
        setup_tmp_repo(&td.path(), &scratch_dir, Some(view));
    }
    else {
        setup_tmp_repo(&td.path(), &scratch_dir, None);
    };

    return call_git(command, td.path(), &args);
}

fn ssh_wrap(command: &str) -> i32
{
    debug!("\n\n############\nssh orig command {:?}", &command);

    let re_cg = Regex::new(r"cg (?P<subcommand>.*)").expect("can't compile regex");
    if let Some(caps) = re_cg.captures(command) {
        let subcommand = caps.name("subcommand").unwrap();
        debug!("cg subcommand: {}", subcommand);
        return cg_command(subcommand);
    }

    let re_git = Regex::new(r"(?P<gitcommand>git-\S*) (?P<args>.*)").expect("can't compile regex");
    if let Some(caps) = re_git.captures(command) {
        let gitcommand = caps.name("gitcommand").unwrap();
        let args = caps.name("args").unwrap();
        return git_command(gitcommand, args);
    }
    return 1;
}

fn update_hook(refname: &str, old: &str, new: &str) -> i32
{
    let scratch = {
        let mut s = String::new();
        File::open(&Path::new("orig"))
            .expect("could not open orig name file")
            .read_to_string(&mut s)
            .expect("could not read orig name");


        let scratch_dir = Path::new(&s);
        let scratch = Scratch::new(&scratch_dir);
        scratch
    };


    let view = {
        let mut s = String::new();
        File::open(&Path::new("view"))
            .expect("could not open view name file")
            .read_to_string(&mut s)
            .expect("could not read view name");

        if s.starts_with(".") {
            return 0;
        }
        let view = SubdirView::new(&Path::new(&s));
        view
    };

    let central_head = scratch.repo.refname_to_id(&refname).expect("no ref: master");


    match scratch.unapply_view(central_head,
                               &view,
                               Oid::from_str(old).expect("can't parse old OID"),
                               Oid::from_str(new).expect("can't parse new OID")) {

        UnapplyView::Done(rewritten) => {
            scratch.repo
                .reference(&refname, rewritten, true, "unapply_view")
                .expect("can't create new reference");
        }
        _ => return 1,
    };

    return 0;
}

fn main_ret() -> i32
{
    let mut args = vec![];
    for arg in env::args() {
        args.push(arg);
    }
    debug!("args: {:?}", args);

    if args[0].ends_with("/update") {
        debug!("================= HOOK {:?}", args);
        return update_hook(&args[1], &args[2], &args[3]);
    }

    if let Ok(command) = env::var("SSH_ORIGINAL_COMMAND") {
        let _lock = FileLock::new("centralgit.lock");
        return ssh_wrap(&command);
    }

    return 1;
}
