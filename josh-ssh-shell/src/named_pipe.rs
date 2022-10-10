extern crate rand;
extern crate libc;

use std::{env, io};
use std::ffi::CString;
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;

const TEMP_SUFFIX_LENGTH: usize = 32;

pub struct NamedPipe {
    path: String
}

impl Drop for NamedPipe {
    fn drop(&mut self) {
        todo!()
    }
}

fn mkfifo(path: &str) -> Result<(), io::Error> {
    let path = CString::new(path).unwrap();

    unsafe {
        libc::mkfifo(path.as_ptr(), 0o660);
    }

    Ok(())
}

pub fn make_named_pipe() -> NamedPipe {
    let temp_path = env::temp_dir();
    let rand_string: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(TEMP_SUFFIX_LENGTH)
        .map(char::from)
        .collect();

    todo!()
}
