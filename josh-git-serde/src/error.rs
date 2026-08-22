use std::fmt;

#[derive(Debug, thiserror::Error)]
#[error("git serde error: {0}")]
pub struct SerdeGitError(pub String);

impl serde::ser::Error for SerdeGitError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        SerdeGitError(msg.to_string())
    }
}

impl serde::de::Error for SerdeGitError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        SerdeGitError(msg.to_string())
    }
}
