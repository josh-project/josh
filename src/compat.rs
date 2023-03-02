pub trait GitOxideCompatExt<T> {
    fn to_git2(self) -> T;
}

impl GitOxideCompatExt<git2::Oid> for gix::ObjectId {
    fn to_git2(self) -> git2::Oid {
        git2::Oid::from_bytes(self.as_slice()).expect("failed to convert oid")
    }
}

pub trait Git2CompatExt<T> {
    fn to_oxide(self) -> T;
}

impl Git2CompatExt<gix::ObjectId> for git2::Oid {
    fn to_oxide(self) -> gix::ObjectId {
        gix::ObjectId::from(self.as_bytes())
    }
}
