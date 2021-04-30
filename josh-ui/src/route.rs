use super::*;

#[derive(Switch, Clone, PartialEq)]
pub enum AppRoute {
    #[to = "/~/{*}/{*}@{*}({*})/{*}[{*}]"]
    Browse(String, String, String, String, String, String),
}

impl AppRoute {
    pub fn with_path(&self, path: &str) -> Self {
        match self.clone() {
            Self::Browse(mode, repo, rev, filter, _, meta) => {
                Self::Browse(mode, repo, rev, filter, path.to_string(), meta)
            }
        }
    }

    pub fn path_up(&self) -> Self {
        match self.clone() {
            Self::Browse(mode, repo, rev, filter, path, meta) => {
                let p = std::path::PathBuf::from(path);
                Self::Browse(
                    mode,
                    repo,
                    rev,
                    filter,
                    p.parent()
                        .map(|x| x.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    meta,
                )
            }
        }
    }

    pub fn edit_filter(&self) -> Self {
        match self.clone() {
            Self::Browse(_, repo, rev, filter, _path, _meta) => {
                Self::Browse("filter".to_string(), repo, rev, filter, _path, _meta)
            }
        }
    }

    pub fn filename(&self) -> String {
        match self.clone() {
            Self::Browse(_, _, _, _, path, _) => {
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
            Self::Browse(_mode, repo, rev, _, path, meta) => Self::Browse(
                "browse".to_string(),
                repo,
                rev,
                filter.to_string(),
                path,
                meta,
            ),
        }
    }

    pub fn with_rev(&self, rev: &str) -> Self {
        match self.clone() {
            Self::Browse(mode, repo, _, filter, path, meta) => {
                Self::Browse(mode, repo, rev.to_string(), filter, path, meta)
            }
        }
    }

    pub fn repo(&self) -> String {
        match self.clone() {
            Self::Browse(_, repo, _, _, _, _) => repo,
        }
    }

    pub fn rev(&self) -> String {
        match self.clone() {
            Self::Browse(_, _, rev, _, _, _) => rev,
        }
    }

    pub fn filter(&self) -> String {
        match self.clone() {
            Self::Browse(_, _, _, filter, _, _) => filter,
        }
    }

    pub fn path(&self) -> String {
        match self.clone() {
            Self::Browse(_, _, _, _, path, _) => path,
        }
    }

    pub fn meta(&self) -> String {
        match self.clone() {
            Self::Browse(_, _, _, _, _, meta) => meta,
        }
    }

    pub fn mode(&self) -> &str {
        match &self {
            Self::Browse(mode, _, _, _, _, _) => &mode,
        }
    }
}

pub type AppAnchor = yew_router::components::RouterAnchor<AppRoute>;
