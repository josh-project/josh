#![deny(warnings)]
extern crate josh;
use josh::virtual_repo;
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

    if args[0].ends_with("/update") {
        println!("josh-proxy");
        exit(virtual_repo::update_hook(&args[1], &args[2], &args[3]));
    }
    exit(josh::run_proxy(args));
}
