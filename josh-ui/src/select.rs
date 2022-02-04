use super::*;
use crate::route::AppRoute;
use crate::select::UrlCheckResult::{ProtocolNotSupported, RemoteFound, RemoteMismatch};
use yew::format::{Nothing, Text};
use yew::{Callback, InputData};
use yew_router::agent::RouteRequest;
use yew_router::route::Route;

pub struct RepoSelector {
    router: RouteAgentDispatcher,
    link: ComponentLink<Self>,
    fetch_task: Option<FetchTask>,
    remote: Option<String>,
    repo: Option<String>,
    hint: Option<String>,
}

pub enum Msg {
    CallServer,
    ReceiveResponse(Text),
    InputChanged(String),
    ChangeRoute,
}

enum UrlCheckResult {
    RemoteMismatch,
    ProtocolNotSupported,
    RemoteFound(String),
}

impl RepoSelector {
    fn check_url(&self, value: &str) -> UrlCheckResult {
        if self.remote.is_none() {
            return RemoteMismatch;
        }

        let remote = self.remote.as_ref().unwrap();

        return if value.starts_with(remote.as_str()) {
            RemoteFound(value.trim_start_matches(remote.as_str()).to_string())
        } else if value.starts_with("git@") {
            ProtocolNotSupported
        } else {
            RemoteMismatch
        };
    }
}

impl Component for RepoSelector {
    type Message = Msg;
    type Properties = ();

    fn create(props: Self::Properties, link: ComponentLink<Self>) -> Self {
        let router = RouteAgentDispatcher::new();
        link.send_message(Self::Message::CallServer);
        Self {
            link,
            router,
            fetch_task: None,
            repo: None,
            remote: None,
            hint: None,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Self::Message::CallServer => {
                let request = Request::get("/remote")
                    .body(Nothing)
                    .expect("Could not build request");

                let callback = self.link.callback(|response: Response<Text>| {
                    let data = response.into_body();
                    Self::Message::ReceiveResponse(data)
                });

                let task = FetchService::fetch(request, callback).expect("Failed to start request");
                self.fetch_task = Some(task);

                true
            }
            Self::Message::ReceiveResponse(response) => {
                match response {
                    Ok(value) => {
                        self.remote = Some(value);
                    }
                    Err(error) => {
                        ConsoleService::log(&error.to_string());
                    }
                }

                self.fetch_task = None;
                true
            }
            Self::Message::InputChanged(value) => {
                let path = self.check_url(value.as_str());

                self.hint = Some(match path {
                    RemoteMismatch => {
                        format!("This URL is not hosted on the selected remote")
                    }
                    ProtocolNotSupported => "Only HTTPS access is currently supported".to_string(),
                    RemoteFound(ref s) => {
                        let loc = yew::utils::window().location();
                        format!("Checkout URL: {}{}", loc.origin().unwrap(), s)
                    }
                });

                self.repo = match path {
                    RemoteFound(ref s) => Some(s.clone()),
                    _ => None,
                };

                true
            }
            Self::Message::ChangeRoute => {
                match &self.repo {
                    Some(s) => {
                        let route = Route::new_default_state(format!("/~/browse{}@HEAD(:/)/()", s));
                        self.router.send(RouteRequest::ChangeRoute(route));
                    }
                    _ => {}
                }

                false
            }
        }
    }

    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
        true
    }

    fn view(&self) -> Html {
        let input_callback = self
            .link
            .callback(|event: InputData| Msg::InputChanged(event.value));

        let browse_callback = self.link.callback(|_| Msg::ChangeRoute);

        return html! {
            <div class="ui-modal-container">
                <div class="select-repo ui-modal">
                    <div class="select-repo-header">
                        <h3>{ "Select a repo" }</h3>
                    </div>
                    <div class="select-repo-hint">
                        { match &self.remote {
                            None => "...".to_string(),
                            Some(remote) => format!("Repo URL, starting with {}:", remote.clone()),
                        } }
                    </div>
                    <input class="select-repo-input ui-input" oninput={input_callback} />
                    { if let Some(hint) = &self.hint { html! {
                        <div class="select-repo-hint">
                            <span class="select-repo-hint-icon">
                                { "ℹ" }
                            </span>
                            { hint }
                        </div>
                    } } else {
                        html!{}
                    } }
                    <button class="select-repo-button ui-button" onclick={ browse_callback }>
                        { "Browse" }
                    </button>
                </div>
            </div>
        };
    }
}
