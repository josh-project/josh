#![recursion_limit = "10000"]

use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    fn showme();
}

use yew::format::Json;
use yew::services::fetch::{FetchService, FetchTask, Request, Response};
use yew::services::ConsoleService;
use yew::{html, Component, ComponentLink, Html, Properties, ShouldRender};

use yew_router::{
    agent::RouteAgentDispatcher, agent::RouteRequest, route::Route, router::Router, Switch,
};

mod codemirror;
mod filter;
mod ls;
mod nav;
mod patterns;
mod route;

pub struct App {
    _link: ComponentLink<Self>,
}

impl Component for App {
    type Message = nav::Msg;
    type Properties = ();

    fn create(_props: Self::Properties, link: ComponentLink<Self>) -> Self {
        link.send_message(Self::Message::CallServer);
        Self { _link: link }
    }

    fn update(&mut self, _msg: Self::Message) -> ShouldRender {
        false
    }

    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
        false
    }

    fn view(&self) -> Html {
        html! {
            <Router<route::AppRoute>
                render = Router::render(|switch: route::AppRoute| {
                    html!{<>
                        <nav::Nav route=switch.clone()/>
                        {
                            match switch.mode() {
                                "browse" => html!{
                                    <ls::Nav route=switch.clone()/>
                                },
                                "filter" => html!{
                                    <filter::Nav route=switch.clone()/>
                                },
                                _ => html!{}
                            }
                        }
                        </>
                    }
                })
            />
        }
    }
}

fn main() {
    yew::start_app::<App>();
}
