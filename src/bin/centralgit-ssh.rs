extern crate centralgithook;
extern crate fern;
extern crate git2;
extern crate regex;
extern crate tempdir;

#[macro_use]
extern crate log;

use centralgithook::Scratch;
use centralgithook::Shell;
use regex::Regex;
use std::env::current_exe;
use std::env;
use std::os::unix::fs::symlink;
use centralgithook::module_ref;
use centralgithook::module_ref_root;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::process::exit;
use tempdir::TempDir;

fn main()
{
    exit(main_ret())
}

fn setup_tmp_repo(td: &Path, scratch_dir: &Path, root: &str)
{
    debug!("setup_tmp_repo, root: {:?}", &root);
    let shell = Shell { cwd: td.to_path_buf() };

    let ce = current_exe().expect("can't find path to exe");
    shell.command("mkdir hooks");
    symlink(ce, td.join("hooks").join("update")).expect("can't symlink update hook");

    shell.command(&format!("cp {:?} {:?}", scratch_dir.join("HEAD"), td));
    symlink(scratch_dir.join(root), td.join("refs")).expect("can't symlink refs");
    symlink(scratch_dir.join("objects"), td.join("objects")).expect("can't symlink objects");
    // debug!("{:?}", &shell.command("xterm"));

}

fn cg_command(subcommand: &str) -> i32
{
    // let subcommand = format!("{}", subcommand[0]);
    debug!("Command cg {:?}", &subcommand);
    if subcommand == "log" {

        let _ = if let Ok(status) = Command::new("cat")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .arg("/tmp/centralgit.log")
            .status() {
            debug!("call ok");
            return status.code().unwrap();
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

fn call_git(command: &str, args: &str) -> i32
{
    return if let Ok(status) = Command::new(command)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .args(&make_argvec(&args))
        .status() {
        debug!("call ok");
        status.code().unwrap()
    }
    else {
        debug!("failed to call git");
        1
    };
}

fn git_command(command: &str, args: &str) -> i32
{
    debug!("git_command: {}", command);
    debug!("git_subcommand: {}", args);

    let re_view = Regex::new(r".*'.*[.]git/(?P<view>\S+)'").expect("can't compile regex");
    let view = if let Some(caps) = re_view.captures(&args) {
        let view = caps.name("view").unwrap();
        debug!("view: {}", view);
        Some(view)
    }
    else {
        debug!("no view, full repo");
        None
    };

    let td = TempDir::new("centralgit").expect("failed to create tempdir");

    let re_repo = Regex::new(r"(?P<repo>/.*[.]git\S*)").expect("can't compile regex");
    let args = re_repo.replace_all(args, format!("{:?}", td.path()).trim_matches('"'));
    debug!("git_subcommand rewritten: {}", args);

    let scratch_dir = Path::new("/tmp").join("centralgit_central");
    let scratch = Scratch::new(&scratch_dir);

    let shell = Shell { cwd: scratch_dir.to_path_buf() };


    let root = if let Some(view) = view {
        debug!("{:?}", &shell.command("du -a refs"));
        let master = scratch.repo.refname_to_id("refs/heads/master").expect("no ref: master");
        let commit = scratch.split_subdir(&view, master).expect("can't split subdir");
        let rev = module_ref(&view, "master");
        scratch.repo.reference(
            &rev,
            commit,
            true,
            "subtree_split").expect("can't create reference");
        module_ref_root(&view)
    }
    else {
        format!("{}","refs")
    };

    debug!("{:?}", &shell.command("du -a refs"));
    setup_tmp_repo(&td.path(), &scratch_dir, &root);

    return call_git(command, &args);

}

fn ssh_wrap(command: &str) -> i32
{
    debug!("\n\n############\nssh orig command {:?}", &command);

    let re_cg = Regex::new(r"cg (?P<subcommand>.*)").expect("can't compile regex");
    let re_git = Regex::new(r"(?P<gitcommand>git-\S*) (?P<args>.*)").expect("can't compile regex");

    if let Some(caps) = re_cg.captures(command) {
        let subcommand = caps.name("subcommand").unwrap();
        debug!("cg subcommand: {}", subcommand);
        return cg_command(subcommand);
    }
    if let Some(caps) = re_git.captures(command) {
        let gitcommand = caps.name("gitcommand").unwrap();
        let args = caps.name("args").unwrap();
        return git_command(gitcommand, args);
    }
    return 1;
}

fn update_hook() -> i32
{
    debug!("IN HOOK");
    println!("hello from hook");
    return 0;
}

fn main_ret() -> i32
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

    let mut args = vec![];
    for arg in env::args() {
        args.push(arg);
    }
    debug!("args: {:?}", args);

    if args[0].ends_with("/update") {
        return update_hook();
    }

    if let Ok(command) = env::var("SSH_ORIGINAL_COMMAND") {
        return ssh_wrap(&command);
    }

    return 1;
}
