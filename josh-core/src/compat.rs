pub trait GitOxideCompatExt<T> {
    fn to_git2(self) -> T;
}

impl GitOxideCompatExt<git2::Oid> for gix::ObjectId {
    fn to_git2(self) -> git2::Oid {
        git2::Oid::from_bytes(self.as_slice()).expect("failed to convert oid")
    }
}
