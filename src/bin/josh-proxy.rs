extern crate josh;
use std::env;
use std::process::exit;

fn main() {
    let args = {
        let mut args = vec![];
        for arg in env::args() {
            args.push(arg);
        }
        args
    };

    exit(josh::run_proxy(args));
}
