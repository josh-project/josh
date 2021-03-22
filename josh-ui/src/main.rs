#![recursion_limit = "10000"]

use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    fn showme();
}

use js_sys::Date;
use yew::format::{Json, Nothing};
use yew::services::fetch::{FetchService, FetchTask, Request, Response};
use yew::services::ConsoleService;
use yew::{html, Component, ComponentLink, Html, Properties, ShouldRender};

use yew_router::{
    agent::RouteAgentDispatcher, agent::RouteRequest, route::Route, router::Router, Switch,
};

mod nav;
mod route;

pub struct App {
    link: ComponentLink<Self>,
    fetch_task: Option<FetchTask>,
    value: i64,
    error: Option<String>,
    repo: String,
}

fn column() -> Html {
    let title = "Directories";
    let elems: Vec<&str> = vec!["foo", "bar"];
    html! {
        <div class="column">
            <h2> { title } </h2>
            <table class="pathlist"> { for elems.iter().map(|e| {
                html! {
                    <tr data-path=e>
                        <td>
                            <span class="path">{ e }</span>
                        </td>
                    </tr>
                }
            })}</table>
        </div>
    }
}

impl Component for App {
    type Message = nav::Msg;
    type Properties = ();

    fn create(_props: Self::Properties, link: ComponentLink<Self>) -> Self {
        link.send_message(Self::Message::CallServer);
        Self {
            link,
            value: 0,
            fetch_task: None,
            error: None,
            repo: "bsw/central".to_string(),
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        false
    }

    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
        false
    }

    fn view(&self) -> Html {
        html! {
            <Router<route::AppRoute>
                render = Router::render(|switch: route::AppRoute| {
                    match switch {
                        route::AppRoute::Browse(repo, r, f, p) => html!{
                            <>
                            <nav::Nav repo=repo rev=r filter=f file=p/>
                            </>
                        }
                    }
                })
            />
        }
    }
}

impl App {
    fn view_list(&self) -> Html {
        html! {
            <div id="pathlist" class="dirmode loaded">
            { column() }
            </div>
        }
    }

    fn view_file(&self) -> Html {
        html! {
            <div class="filemode loaded" id="codeview"></div>
        }
    }

    fn view_error(&self) -> Html {
        if let Some(error) = &self.error {
            html! {
                <h1> { "Error: " } { error } </h1>
            }
        } else {
            html! {}
        }
    }

    fn view_loading(&self) -> Html {
        if self.fetch_task.is_some() {
            html! {
                <div class="loading">
                    <div class="loader"> { "Loading..." } </div>
                </div>
            }
        } else {
            html! {}
        }
    }
}

fn main() {
    yew::start_app::<App>();
}
