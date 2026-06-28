use crate::filter::Filter;
use crate::op::Op;
use crate::persist::peel_op_ref;

fn normalize_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ std::path::Component::Prefix(..)) = components.peek().cloned() {
        components.next();
        std::path::PathBuf::from(c.as_os_str())
    } else {
        std::path::PathBuf::new()
    };

    for component in components {
        match component {
            std::path::Component::Prefix(..) => unreachable!(),
            std::path::Component::RootDir => {
                ret.push(component.as_os_str());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                ret.pop();
            }
            std::path::Component::Normal(c) => {
                ret.push(c);
            }
        }
    }
    ret
}

pub(super) fn src_path(filter: Filter) -> std::path::PathBuf {
    src_path2(peel_op_ref(filter))
}

fn src_path2(op: &Op) -> std::path::PathBuf {
    normalize_path(&match op {
        Op::Subdir(path) => path.to_owned(),
        Op::File(_, source_path) => source_path.to_owned(),
        Op::Chain(filters) => filters
            .iter()
            .fold(std::path::PathBuf::new(), |acc, f| acc.join(src_path(*f))),
        _ => std::path::PathBuf::new(),
    })
}

pub(super) fn dst_path(filter: Filter) -> std::path::PathBuf {
    dst_path2(peel_op_ref(filter))
}

fn dst_path2(op: &Op) -> std::path::PathBuf {
    normalize_path(&match op {
        Op::Prefix(path) => path.to_owned(),
        Op::File(dest_path, _) => dest_path.to_owned(),
        Op::Chain(filters) => filters
            .iter()
            .rev()
            .fold(std::path::PathBuf::new(), |acc, f| acc.join(dst_path(*f))),
        _ => std::path::PathBuf::new(),
    })
}
