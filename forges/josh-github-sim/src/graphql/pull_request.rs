use juniper::graphql_object;

use super::context::Context;

#[derive(juniper::GraphQLEnum)]
pub(crate) enum PullRequestState {
    OPEN,
    CLOSED,
    MERGED,
}

#[derive(Clone)]
pub(crate) struct PullRequest {
    pub(crate) id: String,
    pub(crate) number: i32,
    pub(crate) title: String,
    pub(crate) head_ref_oid: String,
    pub(crate) head_ref_name: String,
    pub(crate) base_ref_oid: String,
    pub(crate) base_ref_name: String,
}

#[graphql_object(context = Context)]
impl PullRequest {
    fn id(&self) -> &str {
        &self.id
    }
    fn number(&self) -> i32 {
        self.number
    }
    fn title(&self) -> &str {
        &self.title
    }
    fn head_ref_oid(&self) -> &str {
        &self.head_ref_oid
    }
    fn head_ref_name(&self) -> &str {
        &self.head_ref_name
    }
    fn base_ref_oid(&self) -> &str {
        &self.base_ref_oid
    }
    fn base_ref_name(&self) -> &str {
        &self.base_ref_name
    }

    fn reviews(&self, first: i32, _after: Option<String>, context: &Context) -> ReviewConnection {
        let state = context.state.lock().unwrap();
        let nodes: Vec<Review> = state
            .reviews
            .get(&(self.number as i64))
            .map(|review_list| {
                review_list
                    .iter()
                    .map(|(login, review_state)| Review {
                        author: User {
                            login: login.clone(),
                        },
                        state: review_state.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let nodes = nodes.into_iter().take(first as usize).collect();
        ReviewConnection { nodes }
    }
}

pub(crate) struct PullRequestConnection {
    pub(crate) nodes: Vec<PullRequest>,
    pub(crate) total_count: i32,
}

#[graphql_object(context = Context)]
impl PullRequestConnection {
    fn nodes(&self) -> &[PullRequest] {
        &self.nodes
    }
    fn total_count(&self) -> i32 {
        self.total_count
    }
    fn page_info(&self) -> PageInfo {
        PageInfo {
            has_next_page: false,
            end_cursor: None,
        }
    }
}

struct Review {
    author: User,
    state: String,
}

#[graphql_object(context = Context)]
impl Review {
    fn author(&self) -> &User {
        &self.author
    }
    fn state(&self) -> &str {
        &self.state
    }
}

struct ReviewConnection {
    nodes: Vec<Review>,
}

#[graphql_object(context = Context)]
impl ReviewConnection {
    fn nodes(&self) -> &[Review] {
        &self.nodes
    }
    fn page_info(&self) -> PageInfo {
        PageInfo {
            has_next_page: false,
            end_cursor: None,
        }
    }
}

use super::context::{PageInfo, User};
