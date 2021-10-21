use super::*;

#[derive(Switch, Clone, PartialEq)]
#[to = "/~/{*:mode}/{*:repo}@{*:rev}({*:filter})/{*:path}({*:meta})"]
pub struct AppRoute {
    pub mode: String,
    pub repo: String,
    pub rev: String,
    pub filter: String,
    pub path: String,
    pub meta: String,
}

impl AppRoute {
    pub fn with_path(&self, path: &str) -> Self {
        let mut s = self.clone();
        s.path = path.to_string();
        s
    }

    pub fn path_up(&self) -> Self {
        let mut s = self.clone();
        let p = std::path::PathBuf::from(self.path.clone());
        s.path = p
            .parent()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_default();

        s
    }

    pub fn edit_filter(&self) -> Self {
        let mut s = self.clone();
        s.mode = "filter".to_string();
        s
    }

    pub fn edit_rev(&self) -> Self {
        let mut s = self.clone();
        s.mode = "refs".to_string();
        s
    }

    pub fn filename(&self) -> String {
        let p = std::path::PathBuf::from(self.path.clone());
        p.file_name()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    pub fn breadcrumbs(&self) -> Vec<Self> {
        let mut r = vec![];
        let mut x = self.clone();

        loop {
            if !x.path.is_empty() {
                r.push(x.clone());
            } else {
                break;
            }
            x = x.path_up();
        }
        r
    }

    pub fn with_filter(&self, filter: &str) -> Self {
        let mut s = self.clone();
        s.mode = "browse".to_string();
        s.filter = filter.to_string();
        s
    }

    pub fn with_rev(&self, rev: &str) -> Self {
        let mut s = self.clone();
        s.mode = "browse".to_string();
        s.rev = rev.to_string();
        s
    }
}

pub type AppAnchor = yew_router::components::RouterAnchor<AppRoute>;
