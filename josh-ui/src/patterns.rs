use super::*;
use crate::route::{AppAnchor, AppRoute};
use yew::{html, Html};

#[macro_export]
macro_rules! html_if {
    ($x:expr, $y:block) => {
        if $x {
            $y
        } else {
            html! {}
        }
    };
}

#[macro_export]
macro_rules! html_if_let {
    ($x:pat, $y:expr, $z:block) => {if let $x = $y $z  else { html!{}}}
}

pub enum Msg {}

#[derive(Clone, PartialEq)]
pub struct Warnings {
    pub josh: i64,
    pub misra: i64,
}

#[derive(Properties, Clone, PartialEq)]
pub struct Props {
    pub route: route::AppRoute,
    pub list: Vec<(AppRoute, String, Warnings)>,
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

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        true
    }
    fn change(&mut self, props: Self::Properties) -> ShouldRender {
        true
    }

    fn view(&self) -> Html {
        html! {
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
                        {
                          html_if!(elt.2.misra > 0,
                            { html!{ <span class="marker"> { elt.2.misra } </span>  }}
                          )
                        }
                        {
                          html_if!( elt.2.josh > 0,
                            { html!{ <span class="josh_marker"> { elt.2.josh } </span> } }
                          )
                        }
                        </td></tr>
                        </AppAnchor>
                    }
                })
            }
            </table>
            </div>
        }
    }
}
