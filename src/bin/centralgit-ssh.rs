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

fn main()
{
    exit(main_ret())
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

    let td = TempDir::new("centralgit").expect("failed to create tempdir");
    let shell = Shell { cwd: td.path().to_path_buf() };

    git2::Repository::init(&td.path()).expect("init failed");
    shell.command("echo bla > bla");

    shell.command("git config user.name Christian");
    shell.command("git config user.email initcrash@gmail.com");
    shell.command("git add bla");
    shell.command("git commit -m blabla");

    let mut args = vec![];
    for arg in env::args() {
        args.push(arg);
    }
    let command = env::var("SSH_ORIGINAL_COMMAND").expect("SSH_ORIGINAL_COMMAND not set");

    debug!("args: {:?} orig: {:?}", args, command);

    let mut s = command.split_whitespace();
    let command = format!("{}",s.next().unwrap());
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

    let status = if let Ok(status) = Command::new(command)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .args(&cargs).status() {
        debug!("call ok");
        return status.code().unwrap();
    }
    else {
        debug!("failed to call");
        return 1;
    };

}
