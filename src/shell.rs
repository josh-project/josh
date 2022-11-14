use tracing;

use self::tracing::Level;

use std::path::PathBuf;
use std::process::Command;

pub struct Shell {
    pub cwd: PathBuf,
}

impl Shell {
    pub fn command(&self, cmd: &[&str]) -> (String, String, i32) {
        self.command_env(cmd, &[], &[])
    }

    #[tracing::instrument(skip(self, env_notrace))]
    pub fn command_env(
        &self,
        cmd: &[&str],
        env: &[(&str, &str)],
        env_notrace: &[(&str, &str)],
    ) -> (String, String, i32) {
        let git_dir = if self.cwd.join(".git").exists() {
            self.cwd.join(".git")
        } else {
            self.cwd.to_path_buf()
        };

        let env = env.to_owned();
        let env_notrace = env_notrace.to_owned();

        let (cmd, args) = {
            if let [cmd, args @ ..] = cmd {
                (cmd, args)
            } else {
                panic!("No command provided")
            }
        };

        let mut command = Command::new(cmd);
        command
            .current_dir(&self.cwd)
            .args(args)
            .envs(env)
            .envs(env_notrace)
            .env("GIT_DIR", &git_dir);

        let output = command
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

        tracing::event!(Level::TRACE, ?stdout, ?stderr);
        (stdout, stderr, output.status.code().unwrap_or(1))
    }
}
