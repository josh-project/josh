use super::*;

use graphql_client::GraphQLQuery;
use route::AppAnchor;

#[derive(GraphQLQuery)]
#[graphql(schema_path = "josh_api.json", query_path = "nav_query.graphql")]
pub struct NavQuery;

pub enum Msg {
    CallServer,
    ReceiveResponse(Result<nav_query::ResponseData, anyhow::Error>),
}

#[derive(Properties, Clone, PartialEq)]
pub struct Props {
    pub route: route::AppRoute,
}

pub struct Nav {
    link: ComponentLink<Self>,
    _router: RouteAgentDispatcher,
    props: Props,
    fetch_task: Option<FetchTask>,
    data: nav_query::ResponseData,
    error: Option<String>,
}

impl Component for Nav {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        link.send_message(Self::Message::CallServer);
        Self {
            link: link,
            data: nav_query::ResponseData {
                refs: vec![nav_query::NavQueryRefs {
                    name: props.route.rev(),
                }],
                workspaces: nav_query::NavQueryWorkspaces {
                    paths: Some(vec![nav_query::NavQueryWorkspacesPaths {
                        dir: nav_query::NavQueryWorkspacesPathsDir {
                            path: props.route.filter(),
                            rev: nav_query::NavQueryWorkspacesPathsDirRev { warnings: None },
                        },
                    }]),
                },
            },
            props: props,
            error: None,
            fetch_task: None,
            _router: RouteAgentDispatcher::new(),
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Self::Message::CallServer => {
                let query = NavQuery::build_query(nav_query::Variables {
                    rev: self.props.route.rev(),
                });
                let request = Request::post(format!("/~/graphql/{}.git", self.props.route.repo()))
                    .header("Content-Type", "application/json")
                    .body(Json(&query))
                    .expect("Could not build request.");
                let callback = self.link.callback(
                    |response: Response<
                        Json<
                            Result<
                                graphql_client::Response<nav_query::ResponseData>,
                                anyhow::Error,
                            >,
                        >,
                    >| {
                        let Json(data) = response.into_body();
                        Self::Message::ReceiveResponse(data.map(|x| x.data.unwrap()))
                    },
                );
                let task = FetchService::fetch(request, callback).expect("failed to start request");
                self.fetch_task = Some(task);
                true
            }
            Self::Message::ReceiveResponse(response) => {
                match response {
                    Ok(file_content) => {
                        self.data = file_content;
                    }
                    Err(error) => {
                        ConsoleService::log(&error.to_string());
                        self.error = Some(error.to_string())
                    }
                }
                self.fetch_task = None;
                true
            }
        }
    }

    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        if self.props != props {
            self.props = props;
            self.link.send_message(Self::Message::CallServer);
        }
        return true;
    }

    fn view(&self) -> Html {
        let props = &self.props;
        if self.fetch_task.is_some() {
            html! { <div class="loader"> { "Loading..." } </div> }
        } else {
            html! { <div id="pathlist" class="dirmode loaded"> {
                if let Some(workspaces) = &self.data.workspaces.paths { if workspaces.len() != 0 {
                    html!{<div class="column">
                        <h2> { "Workspaces" } </h2>
                        <table class="pathlist">
                            <tr><td><AppAnchor classes="path" route=props.route.with_filter(":/")>{":/"}</AppAnchor></td></tr>
                        { for workspaces.iter().map(|w| { html! {
                            <AppAnchor classes="path" route=props.route.with_filter(&(":workspace=".to_string() + &w.dir.path))> <tr> <td>
                            {
                                    w.dir.path.clone()
                            }
                            </td> </tr> </AppAnchor>
                        }})} </table>
                    </div>}
                } else { html!{} }
                } else { html!{} }
            }
            </div>
            }
        }
    }
}
