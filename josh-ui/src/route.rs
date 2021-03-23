use super::*;

#[derive(Switch, Clone, PartialEq)]
pub enum AppRoute {
    #[to = "/~/browse/{*}@{*}({*})/{*}"]
    Browse(String, String, String, String),
}

impl AppRoute {
    pub fn with_path(&self, path: &str) -> Self {
        match self.clone() {
            Self::Browse(repo, rev, filter, _) => Self::Browse(repo, rev, filter, path.to_string()),
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
        }
    }

    pub fn filename(&self) -> String {
        match self.clone() {
            Self::Browse(repo, rev, filter, path) => {
                let p = std::path::PathBuf::from(path);
                p.file_name()
                    .map(|x| x.to_string_lossy().to_string())
                    .unwrap_or_default()
            }
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
        }
    }

    pub fn with_rev(&self, rev: &str) -> Self {
        match self.clone() {
            Self::Browse(repo, _, filter, path) => {
                Self::Browse(repo, rev.to_string(), filter, path)
            }
        }
    }

    pub fn repo(&self) -> String {
        match self.clone() {
            Self::Browse(repo, _, _, _) => repo,
        }
    }

    pub fn rev(&self) -> String {
        match self.clone() {
            Self::Browse(_, rev, _, _) => rev,
        }
    }

    pub fn filter(&self) -> String {
        match self.clone() {
            Self::Browse(_, _, filter, _) => filter,
        }
    }

    pub fn path(&self) -> String {
        match self.clone() {
            Self::Browse(_, _, _, path) => path,
        }
    }
}

pub type AppAnchor = yew_router::components::RouterAnchor<AppRoute>;
