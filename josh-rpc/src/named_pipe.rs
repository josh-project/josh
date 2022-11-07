extern crate rand;
extern crate libc;

use std::{env, io};
use std::ffi::CString;
use std::io::Error;
use std::path::{Path, PathBuf};
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;

const TEMP_SUFFIX_LENGTH: usize = 32;
const PIPE_CREATE_ATTEMPTS: usize = 10;
const PIPE_FILEMODE: libc::mode_t = 0o660;

pub struct NamedPipe {
    pub path: PathBuf
}

impl Drop for NamedPipe {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).unwrap();
    }
}

impl NamedPipe {
    pub fn new(prefix: &str) -> Result<NamedPipe, io::Error> {
        let created_pipe = try_make_pipe(prefix)?;
        Ok(NamedPipe { path: created_pipe })
    }
}

fn make_fifo(path: &Path) -> Result<(), io::Error> {
    let path_str = path.to_str().unwrap();
    let path = CString::new(path_str).unwrap();
    let return_code = unsafe {
        libc::mkfifo(path.as_ptr(), PIPE_FILEMODE)
    };

    match return_code {
        0 => Ok(()),
        _ => Err(Error::last_os_error())
    }
}

fn make_random_path(prefix: &str) -> PathBuf {
    let temp_path = env::temp_dir();
    let rand_string: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(TEMP_SUFFIX_LENGTH)
        .map(char::from)
        .collect();

    let fifo_name = format!("{}-{}", prefix, rand_string);
    temp_path.join(fifo_name)
}

fn try_make_pipe(prefix: &str) -> Result<PathBuf, io::Error> {
    for _ in 0..PIPE_CREATE_ATTEMPTS {
        let pipe_path = make_random_path(prefix);
        match make_fifo(pipe_path.as_path()) {
            Ok(_) => return Ok(pipe_path),
            Err(e) => match e.kind() {
                io::ErrorKind::AlreadyExists => continue,
                _ => ()
            },
        }
    };

    Err(io::Error::from(io::ErrorKind::AlreadyExists))
}
