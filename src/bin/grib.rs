extern crate grib;
use std::process::exit;
use std::env;

fn main()
{
    let args = {
        let mut args = vec![];
        for arg in env::args() {
            args.push(arg);
        }
        args
    };

    exit(grib::run_proxy(args));
}
