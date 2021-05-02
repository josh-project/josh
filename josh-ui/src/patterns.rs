use super::*;
use crate::route::{AppAnchor, AppRoute};
use yew::{html, Html};

pub enum Msg {}

#[derive(Clone, PartialEq)]
pub struct Warnings {
    pub josh: i64,
    pub misra: i64,
}

#[derive(Properties, Clone, PartialEq)]
pub struct Props {
    pub list: Vec<(AppRoute, String, Warnings)>,
    #[prop_or("".to_string())]
    pub name: String,
    #[prop_or("".to_string())]
    pub suffix: String,
}

pub struct List {
    props: Props,
}

impl Component for List {
    type Message = Msg;
    type Properties = Props;

    fn create(props: Self::Properties, _link: ComponentLink<Self>) -> Self {
        Self { props: props }
    }

    fn update(&mut self, _msg: Self::Message) -> ShouldRender {
        true
    }
    fn change(&mut self, _props: Self::Properties) -> ShouldRender {
        true
    }

    fn view(&self) -> Html {
        if self.props.list.len() > 0 {
            html! {
                <div class="column">
                { if self.props.name.len() > 0 {html!(<h2> { self.props.name.clone() } </h2>)} else{html!()} }
                <div id="pathlist" class="dirmode loaded">
                <table class="pathlist">
                {
                    for self.props.list.iter().map( |elt| {
                        html!{
                            <AppAnchor route={elt.0.clone()}>
                            <tr><td>
                            {
                              elt.1.clone()
                            }
                            { &self.props.suffix }
                            {
                              if elt.2.misra > 0 {
                                  html!{ <span class="marker"> { elt.2.misra } </span>}
                              }
                              else { html!{}}
                            }
                            {
                              if elt.2.josh > 0 {
                                html!{ <span class="josh_marker"> { elt.2.josh } </span> }
                              }
                              else { html!{}}
                            }
                            </td></tr>
                            </AppAnchor>
                        }
                    })
                }
                </table>
                </div>
                </div>
            }
        } else {
            html! {}
        }
    }
}
