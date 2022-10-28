use std::fmt::{self, Display};

use bytes::Bytes;
use http::{HeaderMap, Method};
use reqwest::{Body, Client, Url};
use url::ParseError;
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

#[inline]
fn copy_headers(headers: HeaderMap, dest: &mut HeaderMap) {
    *dest = headers;

    for header in HOP_HEADERS {
        dest.remove(header);
    }
}

#[derive(Debug)]
pub enum IntoReqwestError {
    UrlParseError(ParseError),
}

impl Reject for IntoReqwestError {}

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

    pub fn into_reqwest(self, upstream: &Url) -> Result<reqwest::Request, Rejection> {
        let mut url = upstream
            .join(self.path.as_str())
            .map_err(|e| reject::custom(IntoReqwestError::UrlParseError(e)))?;

        if !self.query.is_empty() {
            url.set_query(Some(self.query.as_str()));
        }

        let mut out = reqwest::Request::new(self.method, url);

        let headers = out.headers_mut();
        copy_headers(self.headers, headers);

        // Remove the host header as it will be set automatically
        headers.remove("host");

        log::trace!("proxy request (request={:#?})", out);

        out.body_mut().replace(Body::from(self.body));

        Ok(out)
    }
}

#[derive(Debug)]
pub struct ReqwestError(reqwest::Error);

impl Reject for ReqwestError {}

impl Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} /{}", self.method, self.path.as_str())
    }
}

pub async fn handle(http: Client, request: Request, upstream: &Url) -> Result<Response, Rejection> {
    let request = request.into_reqwest(upstream)?;
    let response = http
        .execute(request)
        .await
        .map_err(|e| reject::custom(ReqwestError(e)))?;
    let status = response.status();
    let headers = response.headers().clone();

    log::trace!("proxy response (response={:#?})", response);

    let mut proxied_response = Response::new(hyper::Body::wrap_stream(response.bytes_stream()));
    copy_headers(headers, proxied_response.headers_mut());

    *proxied_response.status_mut() = status;

    Ok(proxied_response)
}
