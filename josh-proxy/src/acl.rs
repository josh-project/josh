use toml;
use regex::Regex;
use serde::Deserialize;
use std::{collections::HashMap, result::Result};

#[derive(Clone)]
pub struct Validator {
    rules: HashMap<String, Vec<Regex>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Toml(toml::de::Error),
    Regex(regex::Error),
}

impl Validator {

    pub fn from_toml(src: &str) -> std::result::Result<Validator, Error> {
        // parse Vec<Entry> from text
        let parse: Rule = toml::from_str(&src).map_err(Error::Toml)?;
        // map Vec<Entry> into HashMap<String, Vec<Regex>>
        let rules = parse.rule
            .unwrap_or(Vec::new())
            .into_iter()
            .map(|u| (u.user, u.repo.into_iter().map(|v| Regex::new(v.as_str()).map_err(Error::Regex)).collect::<Result<Vec<Regex>, Error>>()))
            .map(|(w, x)| x.map(|y| (w, y)))
            .collect::<Result<HashMap<String, Vec<Regex>>, Error>>()
            ?;
        Ok(Validator{rules})
    }

    pub fn is_accessible(self: &Validator, user: &str, path: &str) -> bool {
        self.rules.get(user).map(|x| x.iter().any(|r| r.is_match(path))).unwrap_or(true)
    }
}

#[derive(Debug, Deserialize)]
struct Rule {
    pub rule: Option<Vec<Entry>>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    pub user: String,
    pub repo: Vec<String>,
}
