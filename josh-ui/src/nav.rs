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
            link,
            data: nav_query::ResponseData {
                workspaces: nav_query::NavQueryWorkspaces {
                    paths: Some(vec![nav_query::NavQueryWorkspacesPaths {
                        dir: nav_query::NavQueryWorkspacesPathsDir {
                            path: props.route.filter.clone(),
                            rev: nav_query::NavQueryWorkspacesPathsDirRev {
                                warnings: None,
                                dir: None,
                            },
                        },
                    }]),
                },
            },
            props,
            error: None,
            fetch_task: None,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Self::Message::CallServer => {
                let query = NavQuery::build_query(nav_query::Variables {
                    rev: self.props.route.rev.clone(),
                    meta: self.props.route.meta.clone(),
                });
                let request = Request::post(format!("/~/graphql/{}.git", self.props.route.repo))
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
        html! {
            <div class="h">
                <span id="repo">{ &props.route.repo }</span>
                <span id="filter">
                <AppAnchor route=props.route.edit_filter()>
                {props.route.filter.clone()}
                </AppAnchor>
                </span>
                <br/>
                <span class="branch">
                <AppAnchor route=props.route.edit_rev()>
                {props.route.rev.clone()}
                </AppAnchor>
                </span>
                <br/>
                {
                    if "browse" == props.route.mode {
                        html!{
                <div id="breadcrumbs">
                <route::AppAnchor route=props.route.with_path("")><b>{"$ /"}</b></route::AppAnchor>
                {
                    for props.route.breadcrumbs().iter().rev().enumerate().map(|(i, b)| {
                        html! {
                            <>{ if i != 0 {"/"} else {""} }<route::AppAnchor route=b>{ b.filename() }</route::AppAnchor></>
                        }
                    })
                }
                </div>
                        }
                    }
                    else { html!{}}
                }
            </div>
        }
    }
}
