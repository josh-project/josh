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

pub fn list(list: Vec<(AppRoute, Html)>) -> Html {
    html! {
        <div id="pathlist" class="dirmode loaded">
        <table class="pathlist">
        {
            for list.iter().map( |elt| {
                html!{
                    <AppAnchor route={elt.0.clone()}>
                    <tr><td>
                    { elt.1.clone() }
                    </td></tr>
                    </AppAnchor>
                }
            })
        }
        </table>
        </div>
    }
}

pub fn path_with_note(path: &str, num: i64, suffix: Option<&str>) -> Html {
    html! {<>
       {
           if let Some(d) = std::path::Path::new(path).file_name() {
               d.to_string_lossy().to_string()
           } else {
               path.to_string()
           }
       }
       {if let Some(suffix) = suffix { suffix } else { "" }}
       {
           if num > 0 {
               html!{ <span class="marker"> { num } </span> }
           }
           else { html!{} }
       }
    </>}
}
