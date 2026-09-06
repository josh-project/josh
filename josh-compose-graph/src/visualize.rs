use std::fmt::{self, Write as _};

use crate::Graph;

impl Graph {
    /// Return this plan as D2 source.
    pub fn d2(&self) -> impl fmt::Display + '_ {
        D2(self)
    }
}

struct D2<'a>(&'a Graph);

impl fmt::Display for D2<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("direction: down")?;

        for image in self.0.images() {
            write!(
                f,
                "\nimage_{}: \"image {}\"",
                image.oid,
                D2Text(&image.label)
            )?;
        }
        for job in self.0.jobs() {
            write!(f, "\njob_{}: \"{}\"", job.ws_tree, D2Text(&job.meta.label))?;
        }

        for image in self.0.images() {
            for (name, base_oid) in &image.bases {
                write!(
                    f,
                    "\nimage_{base_oid} -> image_{}: \"{}\"",
                    image.oid,
                    D2Text(name)
                )?;
            }
            for (name, input_oid) in &image.inputs {
                write!(
                    f,
                    "\njob_{input_oid} -> image_{}: \"input: {}\"",
                    image.oid,
                    D2Text(name)
                )?;
            }
        }
        for job in self.0.jobs() {
            if let Some(image_oid) = job.meta.image {
                write!(f, "\nimage_{image_oid} -> job_{}: \"image\"", job.ws_tree)?;
            }
            for sidecar in &job.meta.sidecars {
                write!(
                    f,
                    "\nimage_{} -> job_{}: \"sidecar: {}\"",
                    sidecar.image,
                    job.ws_tree,
                    D2Text(&sidecar.name)
                )?;
            }
            for (name, input_oid) in &job.inputs {
                write!(
                    f,
                    "\njob_{input_oid} -> job_{}: \"{}\"",
                    job.ws_tree,
                    D2Text(name)
                )?;
            }
        }

        Ok(())
    }
}

struct D2Text<'a>(&'a str);

impl fmt::Display for D2Text<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut chars = self.0.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\\' => f.write_str("\\\\")?,
                '"' => f.write_str("\\\"")?,
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    f.write_char(' ')?;
                }
                ch if ch.is_control() => f.write_char(' ')?,
                ch => f.write_char(ch)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::{Graph, ImageNode, Job, NetworkPolicy, OutputMode, SidecarSpec, WorkspaceMeta};

    fn oid(digit: u8) -> gix_hash::ObjectId {
        gix_hash::ObjectId::from_hex(&[digit; 40]).unwrap()
    }

    fn meta(
        label: &str,
        image: Option<gix_hash::ObjectId>,
        sidecars: Vec<SidecarSpec>,
    ) -> WorkspaceMeta {
        WorkspaceMeta {
            label: label.to_string(),
            output: OutputMode::Keep,
            cmd: String::new(),
            cache: None,
            network: NetworkPolicy::None,
            image,
            worktree: None,
            sidecars,
        }
    }

    #[test]
    fn d2_includes_all_dependency_types_and_escapes_labels() {
        let base = oid(b'1');
        let image = oid(b'2');
        let sidecar = oid(b'3');
        let dependency = oid(b'4');
        let root = oid(b'5');
        let graph = Graph {
            jobs: vec![
                Job {
                    ws_tree: dependency,
                    meta: meta("dependency", None, vec![]),
                    inputs: vec![],
                    env: vec![],
                },
                Job {
                    ws_tree: root,
                    meta: meta(
                        "root \"<&\\path\r\njob",
                        Some(image),
                        vec![SidecarSpec {
                            name: "db\"one".to_string(),
                            image: sidecar,
                            env: vec![],
                            passthrough: vec![],
                            inject: vec![],
                            port: 5432,
                        }],
                    ),
                    inputs: vec![("source|code".to_string(), dependency)],
                    env: vec![],
                },
            ],
            images: vec![
                ImageNode {
                    oid: base,
                    label: "base \"<&\\path\r\nimage".to_string(),
                    bases: vec![],
                    args: vec![],
                    inputs: vec![],
                    context: None,
                },
                ImageNode {
                    oid: image,
                    label: "build image".to_string(),
                    bases: vec![("BASE|IMAGE".to_string(), base)],
                    args: vec![],
                    inputs: vec![("artifact".to_string(), dependency)],
                    context: None,
                },
                ImageNode {
                    oid: sidecar,
                    label: "sidecar image".to_string(),
                    bases: vec![],
                    args: vec![],
                    inputs: vec![],
                    context: None,
                },
            ],
            job_index: HashMap::new(),
            image_index: HashMap::new(),
        };

        assert_eq!(
            graph.d2().to_string(),
            format!(
                "direction: down\
                 \nimage_{base}: \"image base \\\"<&\\\\path image\"\
                 \nimage_{image}: \"image build image\"\
                 \nimage_{sidecar}: \"image sidecar image\"\
                 \njob_{dependency}: \"dependency\"\
                 \njob_{root}: \"root \\\"<&\\\\path job\"\
                 \nimage_{base} -> image_{image}: \"BASE|IMAGE\"\
                 \njob_{dependency} -> image_{image}: \"input: artifact\"\
                 \nimage_{image} -> job_{root}: \"image\"\
                 \nimage_{sidecar} -> job_{root}: \"sidecar: db\\\"one\"\
                 \njob_{dependency} -> job_{root}: \"source|code\""
            )
        );
    }
}
