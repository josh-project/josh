// Based on https://github.com/gitbutlerapp/gitbutler/blob/eec28854/crates/gitbutler-commit/src/commit_buffer.rs#L7-L10
use bstr::{BStr, BString, ByteSlice, ByteVec};

pub struct CommitBuffer {
    heading: Vec<(BString, BString)>,
    message: BString,
}

impl CommitBuffer {
    pub fn new(buffer: &[u8]) -> Self {
        let buffer = BStr::new(buffer);
        if let Some((heading, message)) = buffer.split_once_str("\n\n") {
            let heading = heading
                .lines()
                .filter_map(|line| line.split_once_str(" "))
                .map(|(key, value)| (key.into(), value.into()))
                .collect();

            Self {
                heading,
                message: message.into(),
            }
        } else {
            Self {
                heading: vec![],
                message: buffer.into(),
            }
        }
    }

    pub fn set_header<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, key: K, value: V) {
        let mut set_heading = false;
        let key: BString = key.as_ref().into();
        let value = value.as_ref();
        self.heading.iter_mut().for_each(|(k, v)| {
            if *k == key {
                *v = value.into();
                set_heading = true;
            }
        });

        if !set_heading {
            self.heading.push((key.into(), value.into()));
        }
    }

    pub fn remove_gpg_signature(&mut self) {
        self.heading.retain(|(k, _)| k != "gpgsig");
    }

    // special handling for parents, because the header can appear multiple times and we want to replace all "parent"
    // headers with new "parent" headers based on provided values, taking care to preserve the position of the headers
    pub fn set_parents(&mut self, new_parents: &[&str]) {
        if new_parents.is_empty() {
            self.heading.retain(|(k, _)| k != "parent");
            return;
        }

        let delete_token = "_delete_";
        let mut insertion_index: usize = 0; // by default, we insert at the start of the heading
        let mut new_parents = new_parents.into_iter();

        self.heading
            .iter_mut()
            .enumerate()
            .for_each(|(idx, (k, v))| {
                if k == "tree" {
                    insertion_index = idx + 1;
                } else if k == "parent" {
                    if let Some(new_parent) = new_parents.next() {
                        *v = BString::from(*new_parent);
                        insertion_index = idx + 1;
                    } else {
                        *v = BString::from(delete_token);
                    }
                }
            });

        self.heading
            .retain(|(k, v)| k != "parent" || v != delete_token);

        self.heading.splice(
            insertion_index..insertion_index,
            new_parents.map(|p| ("parent".into(), BString::from(*p))),
        );
    }

    pub fn set_committer(&mut self, signature: &git2::Signature) {
        self.set_header("committer", &format_signature(signature));
    }

    pub fn set_author(&mut self, signature: &git2::Signature) {
        self.set_header("author", &format_signature(signature));
    }

    pub fn set_message<B: AsRef<[u8]>>(&mut self, message: B) {
        self.message = message.as_ref().into();
    }

    pub fn as_bstring(&self) -> BString {
        let mut output = BString::new(vec![]);

        for (key, value) in &self.heading {
            output.push_str(key);
            output.push_str(" ");
            output.push_str(value);
            output.push_str("\n");
        }

        output.push_str("\n");

        output.push_str(&self.message);

        output
    }
}
fn format_signature(signature: &git2::Signature) -> Vec<u8> {
    let mut output = vec![];

    let time = signature.when();
    let offset = time.offset_minutes();

    output.push_str(signature.name_bytes());
    output.push_str(" <");
    output.push_str(signature.email_bytes());
    output.push_str("> ");
    output.push_str(format!(
        "{} {}{:02}{:02}",
        time.seconds(),
        time.sign(),
        offset / 60,
        offset % 60
    ));

    output
}

impl From<git2::Buf> for CommitBuffer {
    fn from(git2_buffer: git2::Buf) -> Self {
        Self::new(&git2_buffer)
    }
}

impl From<BString> for CommitBuffer {
    fn from(s: BString) -> Self {
        Self::new(s.as_bytes())
    }
}

impl From<CommitBuffer> for BString {
    fn from(buffer: CommitBuffer) -> BString {
        buffer.as_bstring()
    }
}

#[test]
fn test_commit_buffer() {
    let buffer: CommitBuffer = CommitBuffer::new(b"key value\n\nmessage");
    assert_eq!(buffer.heading, vec![("key".into(), "value".into())]);
    assert_eq!(buffer.message, BString::from("message"));

    assert_eq!(buffer.as_bstring(), BString::from("key value\n\nmessage"));

    let mut buffer = buffer;
    buffer.set_header("key", "new value");
    assert_eq!(buffer.heading, vec![("key".into(), "new value".into())]);
    assert_eq!(buffer.message, BString::from("message"));

    assert_eq!(
        BString::from(buffer),
        BString::from("key new value\n\nmessage")
    );
}

#[test]
fn test_set_parents_setting_when_unset() {
    let mut buffer = CommitBuffer::new(b"key value\n\nmessage");
    buffer.set_parents(&["parent1", "parent2"]);
    assert_eq!(
        buffer.heading,
        vec![
            ("parent".into(), "parent1".into()),
            ("parent".into(), "parent2".into()),
            ("key".into(), "value".into())
        ]
    );
}

#[test]
fn test_set_parents_setting_when_unset_inserts_after_tree() {
    let mut buffer =
        CommitBuffer::new(b"tree 123\ncommitter bob <bob@example.com> 1465496956 +0200\n\nmessage");
    buffer.set_parents(&["parent1", "parent2"]);
    assert_eq!(
        buffer.heading,
        vec![
            ("tree".into(), "123".into()),
            ("parent".into(), "parent1".into()),
            ("parent".into(), "parent2".into()),
            (
                "committer".into(),
                "bob <bob@example.com> 1465496956 +0200".into()
            )
        ]
    );
}

#[test]
fn test_set_parents_unsetting_when_set() {
    let mut buffer = CommitBuffer::new(b"parent original\nkey value\n\nmessage");
    buffer.set_parents(&[]);
    assert_eq!(buffer.heading, vec![("key".into(), "value".into())]);
}

#[test]
fn test_set_parents_updating() {
    let mut buffer = CommitBuffer::new(b"parent original\n\nmessage");
    buffer.set_parents(&["parent1", "parent2"]);
    assert_eq!(
        buffer.heading,
        vec![
            ("parent".into(), "parent1".into()),
            ("parent".into(), "parent2".into()),
        ]
    );
    buffer.set_parents(&["parent3"]);
    assert_eq!(buffer.heading, vec![("parent".into(), "parent3".into()),]);
}

#[test]
fn test_set_parents_updating_preserves_location_as_much_as_possible() {
    let mut buffer = CommitBuffer::new(b"a b\nparent a\nc d\nparent b\ne f\n\nmessage");
    buffer.set_parents(&["parent1", "parent2"]);
    assert_eq!(
        buffer.heading,
        vec![
            ("a".into(), "b".into()),
            ("parent".into(), "parent1".into()),
            ("c".into(), "d".into()),
            ("parent".into(), "parent2".into()),
            ("e".into(), "f".into()),
        ]
    );
    buffer.set_parents(&["parent3"]);
    assert_eq!(
        buffer.heading,
        vec![
            ("a".into(), "b".into()),
            ("parent".into(), "parent3".into()),
            ("c".into(), "d".into()),
            ("e".into(), "f".into()),
        ]
    );
    buffer.set_parents(&["parent1", "parent2"]);
    assert_eq!(
        buffer.heading,
        vec![
            ("a".into(), "b".into()),
            ("parent".into(), "parent1".into()),
            ("parent".into(), "parent2".into()),
            ("c".into(), "d".into()),
            ("e".into(), "f".into()),
        ]
    );
}
