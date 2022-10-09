extern crate clap;
extern crate libc;
extern crate shell_words;

use clap::Parser;
use std::os::unix::fs::FileTypeExt;
use std::{env, fs, process};
use RequestedCommand::{GitReceivePack, GitUploadArchive, GitUploadPack};

#[derive(Parser, Debug)]
#[command(about = "Josh SSH shell")]
struct Args {
    #[arg(short)]
    command: String,
}

fn isatty(stream: libc::c_int) -> bool {
    unsafe { libc::isatty(stream) != 0 }
}

fn die(message: &str) -> ! {
    eprintln!("josh-ssh-shell: {}", message);
    process::exit(1);
}

#[derive(Debug)]
enum RequestedCommand {
    GitUploadPack,
    GitUploadArchive,
    GitReceivePack,
}

fn main() {
    let args = Args::parse();

    if isatty(libc::STDIN_FILENO) || isatty(libc::STDOUT_FILENO) {
        die("cannot be run interactively; exiting")
    }

    let command_words = shell_words::split(&args.command).unwrap_or_else(|_| {
        die("parse error; exiting");
    });

    // Check that SSH_AUTH_SOCK is provided and it is a socket
    let auth_sock_path = env::var("SSH_AUTH_SOCK").unwrap_or_else(|_| {
        die("SSH_AUTH_SOCK is not set");
    });

    let sock_metadata = fs::metadata(&auth_sock_path)
        .unwrap_or_else(|_| die("path in SSH_AUTH_SOCK does not exist"));

    if !sock_metadata.file_type().is_socket() {
        die("path in SSH_AUTH_SOCK is not a socket")
    }

    // Convert vector of String to vector of str
    let command_words: Vec<_> = command_words.iter().map(String::as_str).collect();

    let (command, args) = match command_words.as_slice() {
        ["git-upload-pack", rest @ ..] | ["git", "upload-pack", rest @ ..] => (GitUploadPack, rest),
        ["git-upload-archive", rest @ ..] | ["git", "upload-archive", rest @ ..] => {
            (GitUploadArchive, rest)
        }
        ["git-receive-pack", rest @ ..] | ["git", "receive-pack", rest @ ..] => {
            (GitReceivePack, rest)
        }
        _ => die("unknown command"),
    };

    eprintln!("{:?} {:?}", command, args);
}
