use std::io;

use futures_util::{TryStream, TryStreamExt};
use rocket::{
    http::ContentType,
    response::{self, Responder},
    Request, Response,
};
use tokio_util::compat::FuturesAsyncReadCompatExt;

#[derive(Debug, Clone)]
pub struct ByteStream<S>(pub S);

impl<S> From<S> for ByteStream<S>
where
    S: TryStream<Error = io::Error>,
    S::Ok: AsRef<[u8]>,
{
    fn from(stream: S) -> Self {
        ByteStream(stream)
    }
}

impl<'r, S> Responder<'r, 'r> for ByteStream<S>
where
    S: TryStream<Error = io::Error> + Send + 'r,
    S::Ok: AsRef<[u8]> + Send,
{
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'r> {
        Response::build()
            .header(ContentType::Binary)
            .streamed_body(self.0.into_async_read().compat())
            .ok()
    }
}
