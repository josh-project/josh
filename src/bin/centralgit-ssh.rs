extern crate tempdir;
extern crate fern;
extern crate centralgithook;
extern crate git2;

#[macro_use]
extern crate log;
use std::env;
use std::path::Path;
use tempdir::TempDir;
use centralgithook::Shell;
use std::process::Command;
use std::process::Stdio;
use std::process::exit;
use std::os::unix::fs::symlink;
use std::env::current_exe;
use centralgithook::Scratch;

fn main()
{
    exit(main_ret())
}

fn setup_tmp_repo(td: &Path, scratch_dir: &Path)
{

    let shell = Shell { cwd: td.to_path_buf() };

    let ce = current_exe().expect("can't find path to exe");
    shell.command("mkdir hooks");
    symlink(ce, td.join("hooks").join("update")).expect("can't symlink update hook");

    shell.command(&format!("cp {:?} {:?}", scratch_dir.join("HEAD"), td));
    symlink(scratch_dir.join("refs"), td.join("refs")).expect("can't symlink refs");
    symlink(scratch_dir.join("objects"), td.join("objects")).expect("can't symlink objects");
    // debug!("{:?}", &shell.command("xterm"));

}

fn cg_command(s: &Vec<String>) -> i32
{
    let subcommand = format!("{}", s[0]);
    debug!("Command cg {:?}", &subcommand);
    return 0;
}

fn git_command(command: &str, argvec: &Vec<String>) -> i32
{
    let scratch_dir = Path::new("/tmp").join("centralgit_central");
    let scratch = Scratch::new(&scratch_dir);

    let td = TempDir::new("centralgit").expect("failed to create tempdir");

    setup_tmp_repo(&td.path(), &scratch_dir);

    let mut gargs = vec![];

    for c in argvec {
        if c.starts_with("--") {
            gargs.push(format!("{}",c));
        }
        else {
            gargs.push(format!("{:?}", td.path()).trim_matches('"').to_string());
        }
    }

    debug!("gargs: {:?}", gargs);

    let _ = if let Ok(status) = Command::new(command)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .args(&gargs)
        .status() {
        debug!("call ok");
        return status.code().unwrap();
    }
    else {
        debug!("failed to call");
        return 1;
    };
}

fn ssh_wrap(command: &str) -> i32
{
    debug!("orig: {:?}", command);

    let mut s = command.split_whitespace();
    let mut argvec = vec![];

    let command = format!("{}", s.next().unwrap());
    for c in s {
        argvec.push(format!("{}", c));
    }

    debug!("command {:?}", &command);
    if command == "cg" {
        return cg_command(&argvec);
    }
    else {
        return git_command(&command, &argvec);
    }
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
