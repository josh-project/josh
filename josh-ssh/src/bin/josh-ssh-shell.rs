extern crate clap;
extern crate libc;
extern crate shell_words;

use clap::Parser;
use std::process;

#[derive(Parser, Debug)]
#[command(about = "Josh SSH shell")]
struct Args {
    #[arg(short)]
    command: String
}

fn isatty(stream: libc::c_int) -> bool {
    unsafe {
        libc::isatty(stream) != 0
    }
}

fn main() {
    let args = Args::parse();

    if isatty(libc::STDIN_FILENO) || isatty(libc::STDOUT_FILENO) {
        eprintln!("josh-ssh-shell cannot be run interactively; exiting");
        process::exit(1);
    }

    let command_args = shell_words::split()

    println!("{:?}", args);
}
