use seed::web_sys::console;
use seed::{prelude::*, *};
use serde::{Deserialize, Serialize};

// ------ ------
//     Init
// ------ ------

fn init(url: Url, orders: &mut impl Orders<Msg>) -> Model {
    let mut url = url;
    match url.next_path_part() {
        Some("review") => match url.next_path_part() {
            Some(c) => {
                orders.send_msg(Msg::FetchData(c.to_string()));
            }
            _ => {}
        },
        _ => {}
    };
    Model::default()
}

// ------ ------
//     Model
// ------ ------

#[derive(Default)]
struct Model {
    change: Option<Change>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Change {
    diff: String,
}

// ------ ------
//    Update
// ------ ------

enum Msg {
    DataFetched(Change),
    FetchData(String),
}

fn update(msg: Msg, model: &mut Model, orders: &mut impl Orders<Msg>) {
    match msg {
        Msg::FetchData(c) => {
            orders.skip();
            let url = format!("/c/{}/", c);
            orders.perform_cmd(async {
                let response = fetch(url).await.expect("fetch failed");

                let change = response
                    .check_status()
                    .expect("check_status failed")
                    .json()
                    .await
                    .expect("deserialization failed");

                Msg::DataFetched(change)
            });
        }
        Msg::DataFetched(change) => {
            model.change = Some(change);
            orders.after_next_render(|_| {
                console::time_end_with_label("rendering");
            });
        }
    }
}

fn unified_diff(diff: &str) -> Node<Msg> {
    // let mut current_ofile = "".to_string();
    // let mut current_nfile = "".to_string();

    let mut arr = vec![];

    let mut oline = 0;
    let mut nline = 0;
    for value in diff.split("\n") {
        if value.starts_with("@") {
            continue;
        }
        if value.starts_with("+++") {
            continue;
        }
        if value.starts_with("---") {
            continue;
        }
        // if value.starts_with("index") {
        //     if let [idx, ff, rest] = value.split(" ").collect::<Vec<_>>().as_slice() {
        //         if let [o, n] = ff.split("..").collect::<Vec<_>>().as_slice() {
        //             current_ofile = o.to_string();
        //             current_nfile = n.to_string();
        //         }
        //     }
        //     continue;
        // }
        if value.starts_with("diff --git") {
            if let [_d, _g, a, b] =
                value.split(" ").collect::<Vec<_>>().as_slice()
            {
                oline = 1;
                nline = 1;
                arr.push(tr![
                    C!["head"],
                    td![attrs!(At::ColSpan=> 3), pre![""]]
                ]);
                arr.push(tr![
                    C!["head"],
                    td![
                        attrs!(At::ColSpan=> 3),
                        pre![format!("{} -> {}", &a[2..], &b[2..])]
                    ]
                ]);
                arr.push(tr![
                    C!["head"],
                    td![attrs!(At::ColSpan=> 3), pre![""]]
                ]);
                continue;
            }
        }
        if value.starts_with(" ") {
            arr.push(code_line("", Some(oline), Some(nline), &value[1..]));
            oline += 1;
            nline += 1;
        } else if value.starts_with("+") {
            arr.push(code_line("addition", None, Some(nline), &value[1..]));
            nline += 1;
        } else if value.starts_with("-") {
            arr.push(code_line("removal", Some(oline), None, &value[1..]));
            oline += 1;
        }
    }

    table![tbody![arr]]
}

fn code_line(
    cls: &str,
    oline: Option<i32>,
    nline: Option<i32>,
    value: &str,
) -> Node<Msg> {
    tr![
        C![cls],
        td![C!["linenr"], pre![oline],],
        td![C!["linenr"], pre![nline],],
        td![C!["code"], pre![value],]
    ]
}

// ------ ------
//     View
// ------ ------

fn view(model: &Model) -> Node<Msg> {
    if let Some(change) = &model.change {
        log!("view with `change` invoked");

        console::time_with_label("diffing");
        let diff_html = unified_diff(&change.diff);
        console::time_end_with_label("diffing");

        console::time_with_label("rendering");
        diff_html
    } else {
        div!["No diff"]
    }
}

// ------ ------
//     Start
// ------ ------

#[wasm_bindgen(start)]
pub fn start() {
    App::start("app", init, update, view);
}
