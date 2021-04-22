use super::*;

#[derive(Switch, Clone, PartialEq)]
pub enum AppRoute {
    #[to = "/~/browse/{*}@{*}({*})/{*}"]
    Browse(String, String, String, String),
    #[to = "/~/filter/{*}@{*}({*})"]
    Filter(String, String, String),
}

impl AppRoute {
    pub fn with_path(&self, path: &str) -> Self {
        match self.clone() {
            Self::Browse(repo, rev, filter, _) => Self::Browse(repo, rev, filter, path.to_string()),
            _ => self.clone(),
        }
    }

    pub fn path_up(&self) -> Self {
        match self.clone() {
            Self::Browse(repo, rev, filter, path) => {
                let p = std::path::PathBuf::from(path);
                Self::Browse(
                    repo,
                    rev,
                    filter,
                    p.parent()
                        .map(|x| x.to_string_lossy().to_string())
                        .unwrap_or_default(),
                )
            }
            _ => self.clone(),
        }
    }

    pub fn edit_filter(&self) -> Self {
        match self.clone() {
            Self::Browse(repo, rev, filter, _path) => Self::Filter(repo, rev, filter),
            _ => self.clone(),
        }
    }

    pub fn filename(&self) -> String {
        match self.clone() {
            Self::Browse(_, _, _, path) => {
                let p = std::path::PathBuf::from(path);
                p.file_name()
                    .map(|x| x.to_string_lossy().to_string())
                    .unwrap_or_default()
            }
            _ => "".to_string(),
        }
    }

    pub fn breadcrumbs(&self) -> Vec<Self> {
        let mut r = vec![];
        let mut x = self.clone();

        loop {
            if x.path() != "" {
                r.push(x.clone());
            } else {
                break;
            }
            x = x.path_up();
        }
        return r;
    }

    pub fn with_filter(&self, filter: &str) -> Self {
        match self.clone() {
            Self::Browse(repo, rev, _, path) => Self::Browse(repo, rev, filter.to_string(), path),
            Self::Filter(repo, rev, _) => {
                Self::Browse(repo, rev, filter.to_string(), "".to_string())
            }
        }
    }

    pub fn with_rev(&self, rev: &str) -> Self {
        match self.clone() {
            Self::Browse(repo, _, filter, path) => {
                Self::Browse(repo, rev.to_string(), filter, path)
            }
            Self::Filter(repo, _, filter) => Self::Filter(repo, rev.to_string(), filter),
        }
    }

    pub fn repo(&self) -> String {
        match self.clone() {
            Self::Browse(repo, _, _, _) => repo,
            Self::Filter(repo, _, _) => repo,
        }
    }

    pub fn rev(&self) -> String {
        match self.clone() {
            Self::Browse(_, rev, _, _) => rev,
            Self::Filter(_, rev, _) => rev,
        }
    }

    pub fn filter(&self) -> String {
        match self.clone() {
            Self::Browse(_, _, filter, _) => filter,
            Self::Filter(_, _, filter) => filter,
        }
    }

    pub fn path(&self) -> String {
        match self.clone() {
            Self::Browse(_, _, _, path) => path,
            _ => "".to_string(),
        }
    }
}

pub type AppAnchor = yew_router::components::RouterAnchor<AppRoute>;
