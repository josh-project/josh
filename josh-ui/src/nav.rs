use super::*;

pub enum Msg {
    CallServer,
    ReceiveResponse(Result<BrowseData, anyhow::Error>),
    ChangeFilter(yew::events::ChangeData),
    ChangeRef(yew::events::ChangeData),
    ChangePath(yew::events::ChangeData),
}

#[derive(Properties, Clone, PartialEq)]
pub struct Props {
    pub repo: String,
    pub rev: String,
    pub filter: String,
    pub file: String,
}

pub struct Nav {
    link: ComponentLink<Self>,
    router: RouteAgentDispatcher,
    props: Props,
    fetch_task: Option<FetchTask>,
    data: BrowseData,
    error: Option<String>,
}

impl Component for Nav {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        link.send_message(Self::Message::CallServer);
        Self {
            link: link,
            data: BrowseData {
                data: References {
                    refs: vec![Reference {
                        name: props.rev.clone(),
                    }],
                    workspaces: Workspaces {
                        paths: vec![Path {
                            dir: Dir {
                                path: props.filter.clone(),
                            },
                        }],
                    },
                },
            },
            props: props,
            error: None,
            fetch_task: None,
            router: RouteAgentDispatcher::new(),
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Self::Message::CallServer => {
                let query = format!(
                    r#"{{
                      refs {{ name }}
                      workspaces: rev(at: "{}", filter: "::**/workspace.josh") {{
                        paths: files {{
                          dir(relative: "..") {{ path }}
                        }}
                      }}
                }}"#,
                    self.props.rev
                );
                let body = serde_json::json!({ "query": query });
                let request = Request::post("/~/graphql/bsw/central.git")
                    .header("Content-Type", "application/json")
                    .body(Json(&body))
                    .expect("Could not build request.");
                let callback = self.link.callback(
                    |response: Response<Json<Result<BrowseData, anyhow::Error>>>| {
                        let Json(data) = response.into_body();
                        Self::Message::ReceiveResponse(data)
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
            Self::Message::ChangeFilter(yew::events::ChangeData::Select(val)) => {
                self.router.send(RouteRequest::ChangeRoute(Route::from(
                    route::AppRoute::Browse(
                        self.props.repo.clone(),
                        self.props.rev.clone(),
                        val.value(),
                        self.props.file.clone(),
                    ),
                )));
                true
            }
            Self::Message::ChangeRef(yew::events::ChangeData::Select(val)) => {
                self.router.send(RouteRequest::ChangeRoute(Route::from(
                    route::AppRoute::Browse(
                        self.props.repo.clone(),
                        val.value(),
                        self.props.filter.clone(),
                        self.props.file.clone(),
                    ),
                )));
                true
            }
            Self::Message::ChangePath(yew::events::ChangeData::Value(val)) => {
                self.router.send(RouteRequest::ChangeRoute(Route::from(
                    route::AppRoute::Browse(
                        self.props.repo.clone(),
                        self.props.rev.clone(),
                        self.props.filter.clone(),
                        val,
                    ),
                )));
                true
            }
            _ => {
                ConsoleService::log("???");
                false
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
        let f_cb = self
            .link
            .callback(|val: yew::events::ChangeData| Self::Message::ChangeFilter(val));
        let r_cb = self
            .link
            .callback(|val: yew::events::ChangeData| Self::Message::ChangeRef(val));
        let p_cb = self
            .link
            .callback(|val: yew::events::ChangeData| Self::Message::ChangePath(val));
        html! {
            <div class="h">
                <form spellcheck="false" autocomplete="off" id="revform">
                    <span id="repo">{ &props.repo }</span>
                    <select id="filter" onchange=f_cb>
                        <option value=":/"> { ":/" } </option>
                        {
                            self.data.data.workspaces.paths.iter().map(|x| html! {
                                <option selected=ws(&x.dir.path) == props.filter> { ws(&x.dir.path) } </option>
                            }).collect::<Html>()
                        }
                    </select>
                    <br/>
                    <select id="ref" onchange=r_cb>
                        {
                            self.data.data.refs.iter().map(|x| html! {
                                <option selected=&x.name == &props.rev value=&x.name> { &x.name } </option>
                            }).collect::<Html>()
                        }
                    </select>
                    <input id="filename" value=&props.file onchange=p_cb/>
                    <span id="up"> { "../" }</span>
                </form>
            </div>
        }
    }
}

fn ws(path: &str) -> String {
    if path.starts_with(":") {
        return path.to_string();
    }
    format!(":workspace={}", path)
}

#[derive(serde::Deserialize)]
pub struct Reference {
    name: String,
}

#[derive(serde::Deserialize)]
pub struct Dir {
    path: String,
}

#[derive(serde::Deserialize)]
pub struct Path {
    dir: Dir,
}

#[derive(serde::Deserialize)]
pub struct Workspaces {
    paths: Vec<Path>,
}

#[derive(serde::Deserialize)]
pub struct References {
    refs: Vec<Reference>,
    workspaces: Workspaces,
}

#[derive(serde::Deserialize)]
pub struct BrowseData {
    data: References,
}
