use bytes::Bytes;
use http_body_util::{BodyExt, combinators::BoxBody};
use hyper::Response;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
pub type JoshBody = BoxBody<Bytes, BoxError>;
pub type JoshResponse = Response<JoshBody>;

pub fn empty() -> JoshBody {
    use http_body_util::{BodyExt, Empty};
    return Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed();
}

pub fn full(b: impl Into<Bytes>) -> JoshBody {
    use http_body_util::{BodyExt, Full};
    return Full::<Bytes>::new(b.into())
        .map_err(|never| match never {})
        .boxed();
}

pub fn erase<B>(res: hyper::Response<B>) -> JoshResponse
where
    B: hyper::body::Body<Data = Bytes> + Send + Sync + 'static,
    B::Error: Into<BoxError>,
{
    res.map(|b| b.map_err(Into::into).boxed())
}
