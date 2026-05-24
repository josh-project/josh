use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::types::GraphQLState;

pub(crate) struct Context {
    pub(crate) repos: HashMap<(String, String), PathBuf>,
    pub(crate) state: Arc<Mutex<GraphQLState>>,
}

impl juniper::Context for Context {}

pub(crate) struct User {
    pub(crate) login: String,
}

#[juniper::graphql_object(context = Context)]
impl User {
    fn login(&self) -> &str {
        &self.login
    }
}

pub(crate) struct PageInfo {
    pub(crate) has_next_page: bool,
    pub(crate) end_cursor: Option<String>,
}

#[juniper::graphql_object(context = Context)]
impl PageInfo {
    fn has_next_page(&self) -> bool {
        self.has_next_page
    }
    fn end_cursor(&self) -> Option<&str> {
        self.end_cursor.as_deref()
    }
}
