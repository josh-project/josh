use async_trait::async_trait;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Middleware(#[from] anyhow::Error),
    #[error(transparent)]
    Command(#[from] std::io::Error),
}

/// A command builder whose args and env can be inspected and mutated by middleware.
pub struct Command {
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    current_dir: Option<std::path::PathBuf>,
}

impl Command {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            current_dir: None,
        }
    }

    /// Set the working directory the command runs in.
    pub fn current_dir(&mut self, dir: impl Into<std::path::PathBuf>) -> &mut Self {
        self.current_dir = Some(dir.into());
        self
    }

    pub fn arg(&mut self, arg: impl Into<String>) -> &mut Self {
        self.args.push(arg.into());
        self
    }

    pub fn args(&mut self, args: impl IntoIterator<Item = impl Into<String>>) -> &mut Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn env(&mut self, key: impl Into<String>, val: impl Into<String>) -> &mut Self {
        self.env.push((key.into(), val.into()));
        self
    }

    pub fn envs(
        &mut self,
        vars: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> &mut Self {
        self.env
            .extend(vars.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    /// Mutable access to the program for middleware
    pub fn program_mut(&mut self) -> &mut String {
        &mut self.program
    }

    /// Mutable access to the argument list for middleware.
    pub fn args_mut(&mut self) -> &mut Vec<String> {
        &mut self.args
    }

    /// Mutable access to the env list for middleware.
    pub fn env_mut(&mut self) -> &mut Vec<(String, String)> {
        &mut self.env
    }

    /// Materialize into a [`tokio::process::Command`].
    pub fn into_tokio(self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.args(&self.args);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        if let Some(dir) = &self.current_dir {
            cmd.current_dir(dir);
        }
        cmd
    }
}

/// Middleware that modifies a [`Command`] before it is spawned.
#[async_trait]
pub trait CommandMiddleware: Send + Sync {
    async fn apply(&self, cmd: &mut Command) -> anyhow::Result<()>;
}

/// Lets a shared `Arc<dyn CommandMiddleware>` be used wherever a middleware is
/// expected, so the same instance can be layered here and used elsewhere.
#[async_trait]
impl<T: CommandMiddleware + ?Sized> CommandMiddleware for std::sync::Arc<T> {
    async fn apply(&self, cmd: &mut Command) -> anyhow::Result<()> {
        (**self).apply(cmd).await
    }
}

/// A stack of [`CommandMiddleware`] layers, applied in order.
pub struct CommandStack {
    layers: Vec<Box<dyn CommandMiddleware>>,
}

impl CommandStack {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Add a middleware layer to the stack.
    pub fn layer<M: CommandMiddleware + 'static>(mut self, middleware: M) -> Self {
        self.layers.push(Box::new(middleware));
        self
    }

    /// Apply all middleware and run the command, returning its output.
    pub async fn run(&self, mut cmd: Command) -> Result<std::process::Output> {
        for layer in &self.layers {
            layer.apply(&mut cmd).await?;
        }

        Ok(cmd.into_tokio().output().await?)
    }
}

impl Default for CommandStack {
    fn default() -> Self {
        Self::new()
    }
}
