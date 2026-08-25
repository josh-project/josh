use gix_object::Find;
use josh_git_serde::{GitValue, from_tree_oid, to_tree_oid};
use josh_gix_ext::StagingOdb;
use std::cell::RefCell;
use std::collections::BTreeMap;

/// Write facade over [`StagingOdb`], which implements only the read side:
/// writes stage in memory exactly like `StagingOdb`'s own `write_*` methods.
struct Stage(RefCell<StagingOdb>);

impl Stage {
    fn new() -> Self {
        Stage(RefCell::new(StagingOdb::new()))
    }

    fn stage(&self, kind: gix_object::Kind, data: Vec<u8>) -> gix_hash::ObjectId {
        self.0.borrow_mut().write_raw(kind, data)
    }
}

impl gix_object::Write for Stage {
    fn write_buf_with_known_id(
        &self,
        kind: gix_object::Kind,
        from: &[u8],
        _id: gix_hash::ObjectId,
    ) -> Result<gix_hash::ObjectId, gix_object::write::Error> {
        Ok(self.stage(kind, from.to_vec()))
    }

    fn write_stream(
        &self,
        kind: gix_object::Kind,
        _size: u64,
        from: &mut dyn std::io::Read,
    ) -> Result<gix_hash::ObjectId, gix_object::write::Error> {
        let mut data = Vec::new();
        from.read_to_end(&mut data)?;
        Ok(self.stage(kind, data))
    }

    fn write_stream_with_known_id(
        &self,
        kind: gix_object::Kind,
        _size: u64,
        from: &mut dyn std::io::Read,
        _id: gix_hash::ObjectId,
    ) -> Result<gix_hash::ObjectId, gix_object::write::Error> {
        let mut data = Vec::new();
        from.read_to_end(&mut data)?;
        Ok(self.stage(kind, data))
    }
}

fn nested_value() -> GitValue {
    GitValue::Tree(BTreeMap::from([
        ("name".into(), Box::new(GitValue::blob_from_str("josh"))),
        ("empty".into(), Box::new(GitValue::empty_blob())),
        (
            "bin".into(),
            Box::new(GitValue::Blob(vec![0xff, 0x00, 0xfe])),
        ),
        (
            "sub".into(),
            Box::new(GitValue::Tree(BTreeMap::from([
                ("leaf".into(), Box::new(GitValue::blob_from_str("deep"))),
                (
                    "subsub".into(),
                    Box::new(GitValue::Tree(BTreeMap::from([(
                        "x".into(),
                        Box::new(GitValue::blob_from_str("42")),
                    )]))),
                ),
            ]))),
        ),
    ]))
}

#[test]
fn roundtrip_nested_tree() {
    let stage = Stage::new();
    let value = nested_value();
    let root = to_tree_oid(&stage, &value).unwrap();
    let back = from_tree_oid(&*stage.0.borrow(), root).unwrap();
    assert_eq!(value, back);
}

#[test]
fn empty_tree_roundtrips() {
    let stage = Stage::new();
    let value = GitValue::Tree(BTreeMap::new());
    let root = to_tree_oid(&stage, &value).unwrap();
    assert_eq!(root.to_string(), "4b825dc642cb6eb9a060e54bf8d69288fbee4904");
    let back = from_tree_oid(&*stage.0.borrow(), root).unwrap();
    assert_eq!(value, back);
}

#[test]
fn blob_root_roundtrips() {
    let stage = Stage::new();
    let value = GitValue::blob_from_str("lone blob");
    let root = to_tree_oid(&stage, &value).unwrap();
    assert_eq!(
        root,
        gix_object::compute_hash(gix_hash::Kind::Sha1, gix_object::Kind::Blob, b"lone blob")
            .unwrap()
    );
    let back = from_tree_oid(&*stage.0.borrow(), root).unwrap();
    assert_eq!(value, back);
}

#[test]
fn written_tree_is_in_canonical_order() {
    // "foo" as a tree sorts as "foo/": after "foo-bar" and "foo.txt".
    // Plain byte order would place it before both.
    let value = GitValue::Tree(BTreeMap::from([
        (
            "foo".into(),
            Box::new(GitValue::Tree(BTreeMap::from([(
                "inner".into(),
                Box::new(GitValue::blob_from_str("i")),
            )]))),
        ),
        ("foo.txt".into(), Box::new(GitValue::blob_from_str("t"))),
        ("foo-bar".into(), Box::new(GitValue::blob_from_str("b"))),
        ("bar".into(), Box::new(GitValue::blob_from_str("a"))),
    ]));
    let stage = Stage::new();
    let root = to_tree_oid(&stage, &value).unwrap();

    let staging = stage.0.borrow();
    let mut buf = Vec::new();
    let (kind, object_hash) = {
        let data = staging.try_find(&root, &mut buf).unwrap().unwrap();
        (data.kind, data.object_hash)
    };
    assert_eq!(kind, gix_object::Kind::Tree);
    let tree = gix_object::TreeRef::from_bytes(&buf, object_hash).unwrap();

    let order: Vec<(bool, String)> = tree
        .entries
        .iter()
        .map(|e| {
            (
                e.mode.is_tree(),
                String::from_utf8(e.filename.to_vec()).unwrap(),
            )
        })
        .collect();
    assert_eq!(
        order,
        vec![
            (false, "bar".to_string()),
            (false, "foo-bar".to_string()),
            (false, "foo.txt".to_string()),
            (true, "foo".to_string()),
        ]
    );

    // Entry oids must resolve to the objects just written.
    for entry in &tree.entries {
        let name: &[u8] = entry.filename.as_ref();
        if name == b"foo" {
            assert_eq!(entry.mode.kind(), gix_object::tree::EntryKind::Tree);
            let sub = from_tree_oid(&*staging, entry.oid.to_owned()).unwrap();
            assert_eq!(
                sub,
                GitValue::Tree(BTreeMap::from([(
                    "inner".into(),
                    Box::new(GitValue::blob_from_str("i")),
                )]))
            );
        } else {
            assert!(
                matches!(name, b"bar" | b"foo-bar" | b"foo.txt"),
                "unexpected entry {name:?}"
            );
            assert_eq!(entry.mode.kind(), gix_object::tree::EntryKind::Blob);
        }
    }
}

#[test]
fn commit_object_is_rejected() {
    let mut staging = StagingOdb::new();
    // Contents are irrelevant: the kind alone must trigger the error.
    let commit = staging.write_raw(gix_object::Kind::Commit, b"tree".to_vec());
    let err = from_tree_oid(&staging, commit).unwrap_err();
    assert!(
        err.to_string().contains("not a tree or a blob"),
        "unexpected error: {err}"
    );
}

#[test]
fn missing_object_is_an_error() {
    let staging = StagingOdb::new();
    let missing = gix_hash::ObjectId::from_bytes_or_panic(&[1; 20]);
    let err = from_tree_oid(&staging, missing).unwrap_err();
    assert!(
        err.to_string().contains("not found"),
        "unexpected error: {err}"
    );
}

fn raw_tree(entries: &[(&str, &str, &gix_hash::ObjectId)]) -> Vec<u8> {
    let mut raw = Vec::new();
    for (mode, name, oid) in entries {
        raw.extend_from_slice(mode.as_bytes());
        raw.push(b' ');
        raw.extend_from_slice(name.as_bytes());
        raw.push(0);
        raw.extend_from_slice(oid.as_bytes());
    }
    raw
}

#[test]
fn executable_blob_entry_is_rejected() {
    let mut staging = StagingOdb::new();
    let blob = staging.write_raw(gix_object::Kind::Blob, b"x".to_vec());
    let tree = staging.write_raw(
        gix_object::Kind::Tree,
        raw_tree(&[("100755", "exec", &blob)]),
    );
    let err = from_tree_oid(&staging, tree).unwrap_err();
    assert!(
        err.to_string().contains("unsupported mode"),
        "unexpected error: {err}"
    );
}

#[test]
fn duplicate_entry_names_are_rejected() {
    let mut staging = StagingOdb::new();
    let a = staging.write_raw(gix_object::Kind::Blob, b"aaa".to_vec());
    let b = staging.write_raw(gix_object::Kind::Blob, b"bbb".to_vec());
    let tree = staging.write_raw(
        gix_object::Kind::Tree,
        raw_tree(&[("100644", "dup", &a), ("100644", "dup", &b)]),
    );
    let err = from_tree_oid(&staging, tree).unwrap_err();
    assert!(
        err.to_string().contains("duplicate entry name"),
        "unexpected error: {err}"
    );
}

#[test]
fn excessive_nesting_is_rejected() {
    let mut value = GitValue::blob_from_str("bottom");
    for _ in 0..1200 {
        value = GitValue::Tree(BTreeMap::from([("n".into(), Box::new(value))]));
    }
    let stage = Stage::new();
    let root = to_tree_oid(&stage, &value).unwrap();
    let err = from_tree_oid(&*stage.0.borrow(), root).unwrap_err();
    assert!(
        err.to_string().contains("nesting exceeds"),
        "unexpected error: {err}"
    );
}

#[test]
fn invalid_entry_names_are_rejected_on_write() {
    for bad in ["", "a/b", "..", "."] {
        let value = GitValue::Tree(BTreeMap::from([(
            bad.to_string(),
            Box::new(GitValue::empty_blob()),
        )]));
        let err = to_tree_oid(&Stage::new(), &value).unwrap_err();
        assert!(
            err.to_string().contains("invalid entry name"),
            "unexpected error for {bad:?}: {err}"
        );
    }
}
