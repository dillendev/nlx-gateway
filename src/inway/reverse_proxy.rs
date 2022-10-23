use std::fmt::{self, Display};

use bytes::Buf;
use futures_util::{Stream, StreamExt};
use http::{HeaderMap, Method};
use reqwest::{Body, Client, Url};
use url::ParseError;
use warp::{
    path::Tail,
    reject::{self, Reject},
    reply::Response,
    Rejection,
};

#[inline]
fn is_hop_header(name: &str) -> bool {
    matches!(
        name,
        "Connection"
            | "Keep-Alive"
            | "Proxy-Authenticate"
            | "Proxy-Authorization"
            | "TE"
            | "Trailers"
            | "Transfer-Encoding"
            | "Upgrade"
    )
}

fn copy_headers(headers: HeaderMap, dest: &mut HeaderMap) {
    let mut header_name = None;

    for (name, value) in headers {
        let name = name.or_else(|| header_name.clone()).unwrap();

        if is_hop_header(name.as_str()) {
            continue;
        }

        dest.append(name.clone(), value);
        header_name = Some(name);
    }
}

#[derive(Debug)]
pub struct UrlParseError(ParseError);

impl Reject for UrlParseError {}

pub struct Request<B: Buf, S: Stream<Item = Result<B, warp::Error>>> {
    method: Method,
    path: Tail,
    query: String,
    headers: HeaderMap,
    body: S,
}

impl<B, S> Request<B, S>
where
    B: Buf,
    S: Stream<Item = Result<B, warp::Error>> + Send + Sync + 'static,
{
    pub fn new(method: Method, path: Tail, query: String, headers: HeaderMap, body: S) -> Self {
        Self {
            method,
            path,
            query,
            headers,
            body,
        }
    }

    pub fn into_reqwest(self, upstream: &str) -> Result<reqwest::Request, Rejection> {
        let mut url = Url::parse(upstream)
            .and_then(|url| url.join(self.path.as_str()))
            .map_err(|e| reject::custom(UrlParseError(e)))?;

        if !self.query.is_empty() {
            url.set_query(Some(self.query.as_str()));
        }

        let mut out = reqwest::Request::new(self.method, url);

        let headers = out.headers_mut();
        copy_headers(self.headers, headers);

        let stream = self
            .body
            .map(|buf| buf.map(|mut buf| buf.copy_to_bytes(buf.remaining())));

        out.body_mut().replace(Body::wrap_stream(stream));

        Ok(out)
    }
}

// Instead of depending on `hyper` directly, use the exported `Body` struct from the `tonic` crate
type HyperBody = tonic::transport::Body;

#[derive(Debug)]
pub struct ReqwestError(reqwest::Error);

impl Reject for ReqwestError {}

impl<B, S> Display for Request<B, S>
where
    B: Buf,
    S: Stream<Item = Result<B, warp::Error>>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} /{}", self.method, self.path.as_str())
    }
}

pub async fn handle<B, S>(
    http: Client,
    request: Request<B, S>,
    upstream: &str,
) -> Result<Response, Rejection>
where
    B: Buf,
    S: Stream<Item = Result<B, warp::Error>> + Send + Sync + 'static,
{
    let request = request.into_reqwest(upstream)?;
    let response = http
        .execute(request)
        .await
        .map_err(|e| reject::custom(ReqwestError(e)))?;
    let status = response.status();
    let headers = response.headers().clone();

    let mut proxy_response = Response::new(HyperBody::wrap_stream(response.bytes_stream()));
    copy_headers(headers, proxy_response.headers_mut());

    *proxy_response.status_mut() = status;

    Ok(proxy_response)
}
