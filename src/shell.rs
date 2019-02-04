extern crate git2;

use std::path::PathBuf;
use std::process::Command;

pub struct Shell {
    pub cwd: PathBuf,
}

impl Shell {
    pub fn command(&self, cmd: &str) -> (String, String) {
        debug!("Shell::command: {}", cmd);
        debug!("cwd: {:?}", self.cwd);
        let git_dir = if self.cwd.join(".git").exists() {
            self.cwd.join(".git")
        } else {
            self.cwd.to_path_buf()
        };
        debug!("GIT_DIR: {:?}", git_dir);

        let output = Command::new("sh")
            .current_dir(&self.cwd)
            .arg("-c")
            .arg(&cmd)
            .env("GIT_DIR", &git_dir)
            .output()
            .unwrap_or_else(|e| panic!("failed to execute process: {}\n{}", cmd, e));

        let stdout = String::from_utf8(output.stdout)
            .expect("failed to decode utf8")
            .trim()
            .to_string();
        let stderr = String::from_utf8(output.stderr)
            .expect("failed to decode utf8")
            .trim()
            .to_string();
        debug!("stdout:\n{}", &stdout);
        debug!("stderr:\n{}", &stderr);
        debug!("\n");
        debug!("\n");
        return (stdout, stderr);
    }
}
