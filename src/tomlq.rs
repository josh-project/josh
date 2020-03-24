use std::collections::HashMap;

fn visit(path: &Vec<String>, v: &toml::Value, res: &mut HashMap<Vec<String>, Vec<toml::Value>>) {
    if v.is_table() {
        for (k, v) in v.as_table().unwrap().iter() {
            let mut path = path.clone();
            path.push(k.to_owned());
            visit(&path, v, res);
        }
        return;
    }
    if v.is_array() {
        if path.last().unwrap_or(&"".to_owned()) == "$all" {
            for v in v.as_array().unwrap().iter() {
                visit(&path, v, res);
            }
            return;
        }
        else {
            for v in v.as_array().unwrap().iter() {
                let mut path = path.clone();
                path.push("$all".to_owned());
                visit(&path, v, res);
            }
        }
    }
    res.entry(path.to_vec())
        .or_insert_with(Vec::new)
        .push(v.clone());
}

pub fn toml_qbe(data: &str, q: &str) -> bool {
           use crate::toml::macros::Deserialize;
    
    let a = data.parse::<toml::Value>().unwrap();
    let q = q.replace("$all", "\"$all\"");

    let mut d = toml::de::Deserializer {
            tokens: toml::Tokenizer::new(&q),
            input: &q,
            require_newline_after_table: true,
            allow_duplciate_after_longer_table: false,
    };
    let b = toml::Value::deserialize(&mut d).unwrap();

    d.end().ok();

    let mut res_data = HashMap::new();
    let mut res_q = HashMap::new();

    visit(&vec![], &a, &mut res_data);
    visit(&vec![], &b, &mut res_q);

    println!("\n{:?}", res_data);
    println!("{:?}", res_q);

    let mut matches = true;

    for (k, vals) in res_q {
        for v in vals {
            let mut m = false;
            res_data.get(&k).map(|entries| {
                for e in entries {
                    if *e == v {
                        m = true;
                    }
                }
            });
            if !m {
                matches = false;
            }
        }
    }

    return matches;
}

#[test]
fn foo() {
    let data = r#"
    [x]
    foo = [1, 2, 3]
    bar = 5
    "#;

    assert!(!toml_qbe(&data, &"y = 4"));
    assert!(!toml_qbe(&data, &"x.foo = [1,3]"));
    assert!(toml_qbe(&data, &""));
    assert!(toml_qbe(&data, &"x.foo = [1,2,3]"));
    assert!(toml_qbe(&data, &"x.foo.$all = 1"));
    assert!(toml_qbe(&data, &"x.foo.$all = [1,2]"));
    assert!(!toml_qbe(&data, &"x.foo.$all = [1,4]"));
}
