use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::{
    body::{Body, Bytes},
    Response,
};

pub fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}
pub fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

pub fn into_box_body<T>(resp: &Response<T>) -> Response<BoxBody<Bytes, hyper::Error>>
where
    T: Body,
    Bytes: From<T>,
{
    resp.map(|body| {
        <Bytes as Into<Bytes>>::into(body)
            .map_err(|e| e.into())
            .boxed()
    })
}
