extern crate git2;
extern crate tracing;

use self::tracing::{event, span, Level};

use std::path::PathBuf;
use std::process::Command;

pub struct Shell {
    pub cwd: PathBuf,
}

impl Shell {
    pub fn command(&self, cmd: &str) -> (String, String) {
        return self.command_env(cmd, &[]);
    }

    pub fn command_env(&self, cmd: &str, env: &[(&str, &str)]) -> (String, String) {
        let git_dir = if self.cwd.join(".git").exists() {
            self.cwd.join(".git")
        } else {
            self.cwd.to_path_buf()
        };
        let _trace_s = span!(Level::TRACE, "shell:command", ?cmd, cwd =?self.cwd, ?git_dir);

        let mut command = Command::new("sh");
        command
            .current_dir(&self.cwd)
            .arg("-c")
            .arg(&cmd)
            .env("GIT_DIR", &git_dir);

        for (k,v) in env.iter() {
            command.env(&k, &v);
        }

        let output = command.output()
            .unwrap_or_else(|e| panic!("failed to execute process: {}\n{}", cmd, e));

        let stdout = String::from_utf8(output.stdout)
            .expect("failed to decode utf8")
            .trim()
            .to_string();
        let stderr = String::from_utf8(output.stderr)
            .expect("failed to decode utf8")
            .trim()
            .to_string();
        event!(Level::TRACE, ?stdout, ?stderr);
        return (stdout, stderr);
    }
}
