extern crate clap;
extern crate libc;
extern crate shell_words;
extern crate josh_ssh_shell;

use clap::Parser;
use std::os::unix::fs::FileTypeExt;
use std::{env, fs, process};
use std::process::ExitCode;
use josh_ssh_shell::named_pipe;
use named_pipe::NamedPipe;

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

async fn handle_command(_command: RequestedCommand, _query: &str) -> Result<(), std::io::Error> {
    let stdout_pipe = NamedPipe::new("josh-stdout")?;
    let stdin_pipe = NamedPipe::new("josh-stdin")?;

    eprintln!("stdout {:?}", stdout_pipe.path);
    eprintln!("stdin {:?}", stdin_pipe.path);

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    let read_stdout = async {
        let mut stdout_pipe_handle = tokio::fs::OpenOptions::new()
            .read(true)
            .write(false)
            .create(false)
            .open(stdout_pipe.path.as_path()).await?;

        tokio::io::copy(&mut stdout_pipe_handle, &mut stdout).await
    };

    let write_stdin = async {
        let mut stdin_pipe_handle = tokio::fs::OpenOptions::new()
            .read(false)
            .write(true)
            .create(false)
            .open(stdin_pipe.path.as_path()).await?;

        tokio::io::copy(&mut stdin, &mut stdin_pipe_handle).await
    };

    tokio::try_join!(read_stdout, write_stdin);

    todo!()
}

#[tokio::main]
async fn main() -> ExitCode {
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
        ["git-upload-pack", rest @ ..] | ["git", "upload-pack", rest @ ..] => {
            (RequestedCommand::GitUploadPack, rest)
        }
        ["git-upload-archive", rest @ ..] | ["git", "upload-archive", rest @ ..] => {
            (RequestedCommand::GitUploadArchive, rest)
        }
        ["git-receive-pack", rest @ ..] | ["git", "receive-pack", rest @ ..] => {
            (RequestedCommand::GitReceivePack, rest)
        }
        _ => die("unknown command"),
    };

    eprintln!("{:?} {:?}", command, args);

    // For now ignore all the extra options those commands can take
    if args.len() != 1 {
        die("invalid arguments supplied for git command")
    }

    let query = args.first().unwrap();

    match handle_command(command, query).await {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("josh-ssh-shell: error: {}", e);
            ExitCode::FAILURE
        }
    }
}
