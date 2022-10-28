use std::{
    fmt::{self, Display},
    str::FromStr,
};

use bytes::Bytes;
use http::{uri::InvalidUri, HeaderMap, Method, Uri};
use hyper::{client::connect::Connect, Body, Client};
use warp::{
    path::Tail,
    reject::{self, Reject},
    reply::Response,
    Rejection,
};

const HOP_HEADERS: [&str; 8] = [
    "Connection",
    "Keep-Alive",
    "Proxy-Authenticate",
    "Proxy-Authorization",
    "TE",
    "Trailers",
    "Transfer-Encoding",
    "Upgrade",
];

#[inline(always)]
fn remove_hop_headers(headers: &mut HeaderMap) {
    for header in HOP_HEADERS {
        headers.remove(header);
    }
}

#[inline]
fn copy_headers(headers: HeaderMap, dest: &mut HeaderMap) {
    *dest = headers;
    remove_hop_headers(dest);
}

#[derive(Debug)]
pub enum IntoRequestError {
    InvalidUri(InvalidUri),
}

impl Reject for IntoRequestError {}

pub struct Request {
    method: Method,
    path: Tail,
    query: String,
    headers: HeaderMap,
    body: Bytes,
}

impl Request {
    pub fn new(method: Method, path: Tail, query: String, headers: HeaderMap, body: Bytes) -> Self {
        Self {
            method,
            path,
            query,
            headers,
            body,
        }
    }

    pub fn into_request(self, upstream: &str) -> Result<hyper::Request<Body>, Rejection> {
        let url = format!("{}{}?{}", upstream, self.path.as_str(), self.query);
        let url = Uri::from_str(&url).map_err(IntoRequestError::InvalidUri)?;

        let mut out = hyper::Request::new(Body::from(self.body));

        *out.method_mut() = self.method;
        *out.uri_mut() = url;

        let headers = out.headers_mut();
        copy_headers(self.headers, headers);

        // Remove the host header as it will be set automatically
        headers.remove("host");

        log::trace!("proxy request (request={:#?})", out);

        Ok(out)
    }
}

#[derive(Debug)]
pub struct HyperError(hyper::Error);

impl Reject for HyperError {}

impl Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} /{}", self.method, self.path.as_str())
    }
}

pub async fn handle<C>(
    http: Client<C>,
    request: Request,
    upstream: &str,
) -> Result<Response, Rejection>
where
    C: Connect + Clone + Send + Sync + 'static,
{
    let request = request.into_request(upstream)?;
    let mut response = http
        .request(request)
        .await
        .map_err(|e| reject::custom(HyperError(e)))?;

    log::trace!("proxy response (response={:#?})", response);

    remove_hop_headers(response.headers_mut());

    Ok(response)
}
