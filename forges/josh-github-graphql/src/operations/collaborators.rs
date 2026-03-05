use crate::connection::GithubApiConnection;

use josh_github_codegen_graphql::{get_repository_collaborators, GetRepositoryCollaborators};

impl GithubApiConnection {
    pub async fn get_maintainers(&self, owner: &str, name: &str) -> anyhow::Result<Vec<String>> {
        let mut maintainers = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let variables = get_repository_collaborators::Variables {
                owner: owner.to_string(),
                name: name.to_string(),
                first: 100,
                after: cursor,
            };

            let response = self
                .make_request::<GetRepositoryCollaborators>(variables)
                .await?;

            let collaborators = response.repository.and_then(|r| r.collaborators);

            let collaborators = match collaborators {
                Some(c) => c,
                None => break,
            };

            if let Some(edges) = collaborators.edges {
                use get_repository_collaborators::RepositoryPermission;
                for edge in edges.into_iter().flatten() {
                    if matches!(
                        edge.permission,
                        RepositoryPermission::Write
                            | RepositoryPermission::Maintain
                            | RepositoryPermission::Admin
                    ) {
                        maintainers.push(edge.node.login);
                    }
                }
            }

            if collaborators.page_info.has_next_page {
                cursor = collaborators.page_info.end_cursor;
            } else {
                break;
            }
        }

        Ok(maintainers)
    }
}
