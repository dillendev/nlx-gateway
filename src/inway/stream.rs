use std::io;

use futures_util::{TryStream, TryStreamExt};
use rocket::{
    http::{HeaderMap, Status},
    response::{self, Responder},
    Request, Response,
};
use tokio_util::compat::FuturesAsyncReadCompatExt;

#[derive(Debug, Clone)]
pub struct ByteStreamResponse<'r, S> {
    stream: S,
    status: Status,
    headers: Option<HeaderMap<'r>>,
}

impl<'r, S> ByteStreamResponse<'r, S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            status: Status::Ok,
            headers: None,
        }
    }

    pub fn set_headers(&mut self, headers: HeaderMap<'r>) {
        self.headers = Some(headers);
    }

    pub fn set_status(&mut self, status: Status) {
        self.status = status;
    }
}

impl<'r, S> From<S> for ByteStreamResponse<'r, S>
where
    S: TryStream<Error = io::Error>,
    S::Ok: AsRef<[u8]>,
{
    fn from(stream: S) -> Self {
        ByteStreamResponse::new(stream)
    }
}

impl<'r, S> Responder<'r, 'r> for ByteStreamResponse<'r, S>
where
    S: TryStream<Error = io::Error> + Send + 'r,
    S::Ok: AsRef<[u8]> + Send,
{
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'r> {
        let mut builder = Response::build();

        if let Some(headers) = self.headers {
            for header in headers.into_iter() {
                builder.header(header);
            }
        }

        builder
            .streamed_body(self.stream.into_async_read().compat())
            .status(self.status)
            .ok()
    }
}
