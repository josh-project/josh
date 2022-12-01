extern crate clap;
extern crate josh_ssh_shell;
extern crate libc;
extern crate shell_words;

use clap::Parser;
use josh_rpc::calls::{RequestedCommand, ServeNamespace};
use josh_rpc::named_pipe::NamedPipe;
use josh_rpc::tokio_fd::IntoAsyncFd;
use reqwest::header::CONTENT_TYPE;
use reqwest::StatusCode;
use std::convert::TryFrom;
use std::fmt::{Display, Formatter};
use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;
use std::{env, fs, io, process};
use tokio::io::AsyncWriteExt;
use tracing_subscriber::Layer;

#[derive(Parser, Debug)]
#[command(about = "Josh SSH shell")]
struct Args {
    #[arg(short)]
    command: String,
}

const HTTP_REQUEST_TIMEOUT: u64 = 300;
const HTTP_JOSH_SERVER_PORT: u16 = 8000;

fn die(message: &str) -> ! {
    eprintln!("josh-ssh-shell: {}", message);
    process::exit(1);
}

#[derive(thiserror::Error, Debug)]
enum CallError {
    FifoError(#[from] std::io::Error),
    RequestError(#[from] reqwest::Error),
    RemoteError { status: StatusCode, body: Vec<u8> },
}

impl Display for CallError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::FifoError(e) => {
                write!(f, "{:?}", e)
            }
            CallError::RequestError(e) => {
                write!(f, "{:?}", e)
            }
            CallError::RemoteError { status, body } => {
                write!(f, "Remote backend returned error: ")?;
                write!(f, "status code: {}, ", status)?;
                write!(f, "body: {}", String::from_utf8_lossy(body).into_owned())
            }
        }
    }
}

fn get_env_int<T: std::str::FromStr>(env_var: &str, default: T) -> T
where
    <T as std::str::FromStr>::Err: Display,
{
    let message = format!(
        "Invalid {} value of env var {}",
        std::any::type_name::<T>(),
        env_var
    );

    env::var(env_var)
        .map(|v| v.parse::<T>().unwrap_or_else(|_| die(&message)))
        .unwrap_or(default)
}

fn get_endpoint() -> String {
    let port = get_env_int("JOSH_SSH_SHELL_ENDPOINT_PORT", HTTP_JOSH_SERVER_PORT);
    format!("http://localhost:{}", port)
}

fn get_timeout() -> u64 {
    get_env_int("JOSH_SSH_SHELL_TIMEOUT", HTTP_REQUEST_TIMEOUT)
}

async fn handle_command(
    command: RequestedCommand,
    ssh_socket: &Path,
    query: &str,
) -> Result<(), CallError> {
    let stdout_pipe = NamedPipe::new("josh-stdout")?;
    let stdin_pipe = NamedPipe::new("josh-stdin")?;

    let stdin_cancel_token = tokio_util::sync::CancellationToken::new();
    let stdin_cancel_token_stdout = stdin_cancel_token.clone();
    let stdin_cancel_token_http = stdin_cancel_token.clone();

    let stdout_cancel_token = tokio_util::sync::CancellationToken::new();
    let stdout_cancel_token_http = stdout_cancel_token.clone();

    let rpc_payload = ServeNamespace {
        command,
        stdout_pipe: stdout_pipe.path.clone(),
        stdin_pipe: stdin_pipe.path.clone(),
        ssh_socket: ssh_socket.to_path_buf(),
        query: query.to_string(),
    };

    let read_stdout = async move {
        let _guard_stdin = stdin_cancel_token_stdout.drop_guard();

        let copy_future = async {
            let mut stdout = josh_rpc::tokio_fd::AsyncFd::try_from(libc::STDOUT_FILENO)?;
            let mut stdout_pipe_handle = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(stdout_pipe.path.as_path())?
                .into_async_fd()?;

            tokio::io::copy(&mut stdout_pipe_handle, &mut stdout).await?;
            stdout.flush().await?;

            Ok(())
        };

        tokio::select! {
            copy_result = copy_future => {
                copy_result.map(|_| ())
            }
            _ = stdout_cancel_token.cancelled() => {
                Ok(())
            }
        }
    };

    let write_stdin = async move {
        let copy_future = async {
            // When the remote end sends EOF over the stdout_pipe,
            // we should stop copying stuff here
            let mut stdin = josh_rpc::tokio_fd::AsyncFd::try_from(libc::STDIN_FILENO)?;
            let mut stdin_pipe_handle = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(stdin_pipe.path.as_path())?
                .into_async_fd()?;

            tokio::io::copy(&mut stdin, &mut stdin_pipe_handle).await?;
            stdin_pipe_handle.flush().await?;

            Ok(())
        };

        tokio::select! {
            copy_result = copy_future => {
                copy_result.map(|_| ())
            }
            _ = stdin_cancel_token.cancelled() => {
                Ok(())
            }
        }
    };

    let make_request = async move {
        let _guard_stdin = stdin_cancel_token_http.drop_guard();
        let _guard_stdout = stdout_cancel_token_http.drop_guard();

        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/serve_namespace", get_endpoint()))
            .header(CONTENT_TYPE, "application/json")
            .body(serde_json::to_string(&rpc_payload).unwrap())
            .timeout(Duration::from_secs(get_timeout()))
            .send()
            .await?;

        let status = response.status();
        let bytes = response.bytes().await?;

        match status {
            StatusCode::OK | StatusCode::NO_CONTENT => Ok(()),
            code => Err(CallError::RemoteError {
                status: code,
                body: bytes.to_vec(),
            }),
        }
    };

    tokio::try_join!(read_stdout, write_stdin, make_request).map(|_| ())
}

fn setup_tracing() {
    let fmt_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_ansi(false)
        .with_writer(io::stderr);

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

    #[cfg(debug_assertions)]
    fn check_isatty() {}

    #[cfg(not(debug_assertions))]
    fn check_isatty() {
        fn isatty(stream: libc::c_int) -> bool {
            unsafe { libc::isatty(stream) != 0 }
        }

        if isatty(libc::STDIN_FILENO) || isatty(libc::STDOUT_FILENO) {
            die("cannot be run interactively; exiting")
        }
    }

    check_isatty();

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
