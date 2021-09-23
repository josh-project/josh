use toml;
use regex::Regex;
use serde::Deserialize;
use std::{collections::HashMap, result::Result};

#[derive(Debug, Clone)]
pub struct Validator {
    // note:       repo            user        paths
    rules: HashMap<String, HashMap<String, Vec<Regex>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Toml(toml::de::Error),
    Regex(regex::Error),
}

impl Validator {

    pub fn from_toml(src: &str) -> std::result::Result<Validator, Error> {
        // parse Rule from text
        let raw: Doc = toml::from_str(&src).map_err(Error::Toml)?;

        // map Rule into HashMap<String, HashMap<String, Vec<Regex>>>
        let dst = |x: Vec<String>| x
            .into_iter()
            .map(|v| Regex::new(v.as_str()).map_err(Error::Regex))
            .collect::<Result<Vec<Regex>, Error>>()
            ;
        let dst = |x: Option<Vec<Match>>| x
            .unwrap_or(Vec::new())
            .into_iter()
            .map(|m| (m.user, dst(m.path)))
            .map(|(s, r)| r.map(|v| (s, v)))
            .collect::<Result<HashMap<String, Vec<Regex>>, Error>>()
            ;
        let dst = raw.repo
            .unwrap_or(Vec::new())
            .into_iter()
            .map(|r| (r.name, dst(r.rule)))
            .map(|(s, r)| r.map(|v| (s, v)))
            .collect::<Result<HashMap<String, _>, Error>>()
            ?;

        // return value
        Ok(Validator{rules: dst})
    }

    pub fn is_accessible(self: &Validator, user: &str, repo: &str, path: &str) -> bool {
        // e.g. if "we" want to access "http://localhost:8080/a/b.git:/c/d.git"
        //      then user = we, repo = a/b, path = c/d
        let repo = repo.trim_end_matches(".git");
        let path = path.trim_start_matches(":/");
        self.rules
            .get(repo)
            .and_then(|r| r.get(user).map(|x| x.iter().any(|r| r.is_match(path))))
            .unwrap_or(false)
    }
}

#[derive(Debug, Deserialize)]
struct Doc {
    pub repo: Option<Vec<Repo>>,
}

#[derive(Debug, Deserialize)]
struct Repo {
    pub name: String,
    pub rule: Option<Vec<Match>>,
}

#[derive(Debug, Deserialize)]
struct Match {
    pub user: String,
    pub path: Vec<String>,
}
