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

fn ssh_wrap(command: &str) -> i32
{
    let scratch_dir = Path::new("/tmp").join("centralgit_central");
    let scratch = Scratch::new(&scratch_dir);

    let td = TempDir::new("centralgit").expect("failed to create tempdir");
    let shell = Shell { cwd: td.path().to_path_buf() };

    let ce = current_exe().expect("can't find path to exe");
    shell.command("mkdir hooks");
    symlink(ce, td.path().join("hooks").join("update")).expect("can't symlink update hook");

    shell.command(&format!("cp {:?} {:?}", scratch_dir.join("HEAD"), td.path()));
    symlink(scratch_dir.join("refs"), td.path().join("refs")).expect("can't symlink refs");
    symlink(scratch_dir.join("objects"), td.path().join("objects")).expect("can't symlink objects");
    // debug!("{:?}", &shell.command("xterm"));

    debug!("orig: {:?}", command);

    let mut s = command.split_whitespace();
    let command = format!("{}", s.next().unwrap());
    let mut cargs = vec![];

    for c in s {
        if c.starts_with("--") {
            cargs.push(format!("{}", c));
        }
        else {
            cargs.push(format!("{:?}", td.path()).trim_matches('"').to_string());
        }
    }

    debug!("cargs: {:?}", cargs);

    let _ = if let Ok(status) = Command::new(command)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .args(&cargs)
        .status() {
        debug!("call ok");
        return status.code().unwrap();
    }
    else {
        debug!("failed to call");
        return 1;
    };
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
