extern crate clap;
extern crate libc;
extern crate shell_words;
extern crate josh_ssh_shell;

use clap::Parser;
use std::os::unix::fs::FileTypeExt;
use std::{env, fs, process};
use std::path::{PathBuf, Path};
use std::process::ExitCode;
use reqwest::header::CONTENT_TYPE;
use tracing_subscriber::Layer;
use josh_ssh_shell::named_pipe;
use named_pipe::NamedPipe;
use josh_rpc::calls::{RequestedCommand, ServeNamespace};

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

async fn handle_command(command: RequestedCommand, ssh_socket: &Path, query: &str) -> Result<(), std::io::Error> {
    let stdout_pipe = NamedPipe::new("josh-stdout")?;
    let stdin_pipe = NamedPipe::new("josh-stdin")?;

    // eprintln!("stdout {:?}", stdout_pipe.path);
    // eprintln!("stdin {:?}", stdin_pipe.path);

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    let read_stdout = async {
        eprintln!("copy: fifo -> own stdout");

        let mut stdout_pipe_handle = tokio::fs::OpenOptions::new()
            .read(true)
            .write(false)
            .create(false)
            .open(stdout_pipe.path.as_path()).await?;

        let result = tokio::io::copy(&mut stdout_pipe_handle, &mut stdout).await;

        eprintln!("copy: fifo -> own stdout finish");

        result
    };

    let write_stdin = async {
        eprintln!("copy: own stdin -> fifo");

        let mut stdin_pipe_handle = tokio::fs::OpenOptions::new()
            .read(false)
            .write(true)
            .create(false)
            .open(stdin_pipe.path.as_path()).await?;

        let result = tokio::io::copy(&mut stdin, &mut stdin_pipe_handle).await;

        eprintln!("copy: own stdin -> fifo finish");

        result
    };

    let rpc_payload = ServeNamespace {
        stdout_pipe: stdout_pipe.path.clone(),
        stdin_pipe: stdin_pipe.path.clone(),
        ssh_socket: ssh_socket.to_path_buf(),
        query: query.to_string(),
    };

    let make_request = async {
        eprintln!("http request");

        let client = reqwest::Client::new();
        let response = client.post("http://localhost:8000/serve_namespace")
            .header(CONTENT_TYPE, "application/json")
            .body(serde_json::to_string(&rpc_payload).unwrap())
            .send()
            .await;

        let result = response.or(Err(std::io::Error::from(std::io::ErrorKind::ConnectionAborted)));

        eprintln!("http request finish");

        result
    };

    let (_, _, response) = tokio::try_join!(read_stdout, write_stdin, make_request)?;

    eprintln!("{:?}", response);

    Ok(())
}

fn setup_tracing() {
    let fmt_layer = tracing_subscriber::fmt::layer().compact().with_ansi(false);

    let filter = match env::var("RUST_LOG") {
        Ok(_) => tracing_subscriber::EnvFilter::from_default_env(),
        _ => tracing_subscriber::EnvFilter::new("josh_ssh_shell=trace"),
    };

    let subscriber = filter
        .and_then(fmt_layer)
        .with_subscriber(tracing_subscriber::Registry::default());

    tracing::subscriber::set_global_default(subscriber).expect("can't set_global_default");
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();

    // if isatty(libc::STDIN_FILENO) || isatty(libc::STDOUT_FILENO) {
    //     die("cannot be run interactively; exiting")
    // }

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

    // For now ignore all the extra options those commands can take
    if args.len() != 1 {
        die("invalid arguments supplied for git command")
    }

    setup_tracing();

    let query = args.first().unwrap();

    match handle_command(command, Path::new(&auth_sock_path), query).await {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("josh-ssh-shell: error: {}", e);
            ExitCode::FAILURE
        }
    }
}
