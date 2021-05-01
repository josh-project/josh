use super::*;

use graphql_client::GraphQLQuery;

#[derive(GraphQLQuery)]
#[graphql(schema_path = "josh_api.json", query_path = "nav_query.graphql")]
pub struct RefsQuery;

pub enum Msg {
    CallServer,
    ReceiveResponse(Result<refs_query::ResponseData, anyhow::Error>),
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
    data: refs_query::ResponseData,
    error: Option<String>,
}

impl Component for Nav {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        link.send_message(Self::Message::CallServer);
        Self {
            link,
            data: refs_query::ResponseData {
                refs: vec![refs_query::RefsQueryRefs {
                    name: props.route.rev.clone(),
                }],
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
                let query = RefsQuery::build_query(refs_query::Variables {});
                let request = Request::post(format!("/~/graphql/{}.git", self.props.route.repo))
                    .header("Content-Type", "application/json")
                    .body(Json(&query))
                    .expect("Could not build request.");
                let callback = self.link.callback(
                    |response: Response<
                        Json<
                            Result<
                                graphql_client::Response<refs_query::ResponseData>,
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
            let mut l = vec![];
            let refs = &self.data.refs;
            {
                if refs.len() != 0 {
                    l.extend(refs.iter().map(|w| {
                        (
                            props.route.with_rev(&w.name),
                            w.name.clone(),
                            patterns::Warnings { misra: 0, josh: 0 },
                        )
                    }));
                }
            };
            html! {<patterns::List name="Branches"  list=l />}
        }
    }
}
