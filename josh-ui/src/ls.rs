use super::*;

use crate::route::AppRoute;
use graphql_client::GraphQLQuery;

#[derive(GraphQLQuery)]
#[graphql(schema_path = "josh_api.json", query_path = "nav_query.graphql")]
pub struct PathQuery;

pub enum Msg {
    CallServer,
    ReceiveResponse(Result<path_query::ResponseData, anyhow::Error>),
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
    data: path_query::ResponseData,
    error: Option<String>,
}

impl Component for Nav {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        link.send_message(Self::Message::CallServer);
        Self {
            link,
            data: path_query::ResponseData {
                rev: path_query::PathQueryRev {
                    warnings: None,
                    file: None,
                    dirs: None,
                    files: None,
                },
            },
            props,
            error: None,
            fetch_task: None,
            _router: RouteAgentDispatcher::new(),
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Self::Message::CallServer => {
                let query = PathQuery::build_query(path_query::Variables {
                    rev: self.props.route.rev.clone(),
                    filter: self.props.route.filter.clone(),
                    path: self.props.route.path.clone(),
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
                                graphql_client::Response<path_query::ResponseData>,
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
        true
    }

    fn view(&self) -> Html {
        let props = &self.props;
        html! {
            <>{
                if self.fetch_task.is_some() {
                    html! { <div class="loader"> { "Loading..." } </div> }
                }
                else if let Some(file) = &self.data.rev.file {
                    html! {<codemirror::Codemirror
                        text=file.text.as_ref().unwrap_or(&"".to_string()).clone()
                        marker_pos=file.meta.data.iter().map(|x| x.position.unwrap_or_default()).collect::<Vec<_>>()
                        marker_text=file.meta.data.iter().map(|x| x.text.clone().unwrap_or_default()).collect::<Vec<String>>()
                    />}
                } else {
                    html! { <>
                        {
                            if let Some(dirs) = &self.data.rev.dirs { html!{
                                <patterns::List name="Directories"  list={
                                    dirs.iter().map(|d| {
                                        (props.route.with_path(&d.path),
                                        filename(&d.path),
                                        patterns::Warnings { josh: 0, misra:d.meta.count})
                                    }).collect::<Vec<(AppRoute, String, patterns::Warnings)>>()
                                } suffix="/"/>
                            }} else { html!{} }
                        }
                        {
                            if let Some(files) = &self.data.rev.files { html!{
                                <patterns::List name="Files"  list={
                                    files.iter().map(|f| {
                                        (props.route.with_path(&f.path),
                                        filename(&f.path),
                                        patterns::Warnings { josh: 0, misra:f.meta.count})
                                    }).collect::<Vec<(AppRoute, String, patterns::Warnings)>>()
                                }/>
                            }} else { html!{} }
                        }
                        {
                            if let Some(warnings) = &self.data.rev.warnings {
                                if !warnings.is_empty() {
                                html! { <>
                                    <div class="warnings">
                                    <h2> { "Warnings" } </h2>
                                    <table>
                                    {
                                        for warnings.iter().map( |warn| {
                                            html! {
                                                <tr><td>
                                                 { &warn.message }
                                                </td></tr>
                                            }
                                        })
                                    }
                                    </table>
                                    </div>
                                    </>
                                }
                                }
                                else { html! {} }
                            }
                            else { html! {} }
                        }
                    </> }
                }
            }</>
        }
    }
}

fn filename(p: &str) -> String {
    if let Some(filename) = std::path::Path::new(p).file_name() {
        filename.to_string_lossy().to_string()
    } else {
        p.to_string()
    }
}
