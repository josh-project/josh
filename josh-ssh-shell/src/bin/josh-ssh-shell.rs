extern crate clap;
extern crate libc;
extern crate shell_words;
extern crate josh_ssh_shell;

use clap::Parser;
use std::os::unix::fs::FileTypeExt;
use std::{env, fs, process};
use std::convert::TryFrom;
use std::fmt::{Display, Formatter};
use std::path::{PathBuf, Path};
use std::process::ExitCode;
use std::time::Duration;
use reqwest::header::CONTENT_TYPE;
use reqwest::StatusCode;
use tokio::io::AsyncWriteExt;
use tracing_subscriber::Layer;
use josh_rpc::calls::{RequestedCommand, ServeNamespace};
use josh_rpc::named_pipe::NamedPipe;

#[derive(Parser, Debug)]
#[command(about = "Josh SSH shell")]
struct Args {
    #[arg(short)]
    command: String,
}

const HTTP_REQUEST_TIMEOUT: u64 = 120;
const HTTP_JOSH_SERVER: &str = "http://localhost:8000";

fn isatty(stream: libc::c_int) -> bool {
    unsafe { libc::isatty(stream) != 0 }
}

fn die(message: &str) -> ! {
    eprintln!("josh-ssh-shell: {}", message);
    process::exit(1);
}

// async fn stdin_thread_test() {
//     std::thread::spawn(move || {
//         logging::info!("Capturing STDIN.");
//
//         loop {
//             let (buffer, len) = match stdin.fill_buf() {
//                 Ok(buffer) if buffer.is_empty() => break, // EOF.
//                 Ok(buffer) => (Ok(Bytes::copy_from_slice(buffer)), buffer.len()),
//                 Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
//                 Err(error) => (Err(error), 0),
//             };
//
//             stdin.consume(len);
//
//             if executor::block_on(sender.send(buffer)).is_err() {
//                 // Receiver has closed so we should shutdown.
//                 break;
//             }
//         }
//     });
// }

#[derive(thiserror::Error, Debug)]
enum CallError {
    FifoError(#[from] std::io::Error),
    RequestError(#[from] reqwest::Error),
    RemoteError {
        status: StatusCode,
        body: Vec<u8>,
    },
}

impl Display for CallError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::FifoError(e) => {
                write!(f, "{:?}", e)
            },
            CallError::RequestError(e) => {
                write!(f, "{:?}", e)
            },
            CallError::RemoteError { status, body } => {
                write!(f, "Remote backend returned error: ")?;
                write!(f, "status code: {}, ", status)?;
                write!(f, "body: {}", String::from_utf8_lossy(body).into_owned())
            }
        }
    }
}

async fn handle_command(command: RequestedCommand, ssh_socket: &Path, query: &str) -> Result<(), CallError> {
    let stdout_pipe = NamedPipe::new("josh-stdout")?;
    let stdin_pipe = NamedPipe::new("josh-stdin")?;

    eprintln!("stdout {:?}", stdout_pipe.path);
    eprintln!("stdin {:?}", stdin_pipe.path);

    let stdin_cancel_token = tokio_util::sync::CancellationToken::new();
    let stdin_cancel_token_stdout = stdin_cancel_token.clone();

    let stdout_cancel_token = tokio_util::sync::CancellationToken::new();
    let stdout_cancel_token_http = stdout_cancel_token.clone();

    let rpc_payload = ServeNamespace {
        stdout_pipe: stdout_pipe.path.clone(),
        stdin_pipe: stdin_pipe.path.clone(),
        ssh_socket: ssh_socket.to_path_buf(),
        query: query.to_string(),
    };

    let read_stdout = async move {
        eprintln!("copy: fifo -> own stdout");

        let copy_future = async {
            let mut stdout = tokio::io::stdout();
            let mut stdout_pipe_handle = tokio::net::UnixStream::connect(stdout_pipe.path.as_path()).await?;

            tokio::io::copy(&mut stdout_pipe_handle, &mut stdout).await?;

            Ok(())
        };

        let result = tokio::select! {
            copy_result = copy_future => {
                copy_result.map(|_| ())
            }
            _ = stdout_cancel_token.cancelled() => {
                Ok(())
            }
        };

        stdin_cancel_token_stdout.cancel();

        eprintln!("copy: fifo -> own stdout finish");

        result
    };

    let write_stdin = async move {
        eprintln!("copy: own stdin -> fifo");

        let copy_future = async {
            // When the remote end sends EOF over the stdout_pipe,
            // we should stop copying stuff here
            let mut stdin = josh_rpc::tokio_fd::AsyncFd::try_from(libc::STDIN_FILENO)?;
            let mut stdin_pipe_handle = tokio::net::UnixStream::connect(stdin_pipe.path.as_path()).await?;

            tokio::io::copy(&mut stdin, &mut stdin_pipe_handle).await?;
            stdin_pipe_handle.flush().await?;

            Ok(())
        };

        let result = tokio::select! {
            copy_result = copy_future => {
                copy_result.map(|_| ())
            }
            _ = stdin_cancel_token.cancelled() => {
                Ok(())
            }
        };

        eprintln!("copy: own stdin -> fifo finish");

        result
    };

    let make_request = async move {
        eprintln!("http request");

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let client = reqwest::Client::new();
        let response = client.post(format!("{}/serve_namespace", HTTP_JOSH_SERVER))
            .header(CONTENT_TYPE, "application/json")
            .body(serde_json::to_string(&rpc_payload).unwrap())
            .timeout(Duration::from_secs(HTTP_REQUEST_TIMEOUT))
            .send()
            .await?;

        let status = response.status();
        let bytes = response
            .bytes()
            .await?;

        let result = match status {
            StatusCode::OK | StatusCode::NO_CONTENT => Ok(()),
            code => Err(CallError::RemoteError {
                status: code,
                body: bytes.to_vec(),
            }),
        };

        eprintln!("http request finish");

        result
    };

    eprintln!("before try_join");
    let request_result = tokio::try_join!(read_stdout, write_stdin, make_request);
    eprintln!("after try_join");

    request_result.map(|_| ())
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
    console_subscriber::init();

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

    // setup_tracing();

    let query = args.first().unwrap();

    match handle_command(command, Path::new(&auth_sock_path), query).await {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("josh-ssh-shell: error: {}", e);
            ExitCode::FAILURE
        }
    }
}
