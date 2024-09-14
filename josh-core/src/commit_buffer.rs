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

    pub fn set_header(&mut self, key: &str, value: &str) {
        let mut set_heading = false;
        self.heading.iter_mut().for_each(|(k, v)| {
            if k == key {
                *v = value.into();
                set_heading = true;
            }
        });

        if !set_heading {
            self.heading.push((key.into(), value.into()));
        }
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

