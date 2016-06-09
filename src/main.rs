extern crate git2;
use git2::*;
use std::process::Command;

fn main() {

  let repo = match Repository::init("/tmp/bla") {
      Ok(repo) => repo,
      Err(e) => panic!("failed to init: {}", e),
  };
  println!("Hello, world!");
}
