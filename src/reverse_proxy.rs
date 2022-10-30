use std::{
    error::Error,
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

const MAX_RETRIES: usize = 3;
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
}

fn create_proxied_request(
    req: &Request,
    upstream: &str,
) -> Result<hyper::Request<Body>, Rejection> {
    let url = format!("{}{}?{}", upstream, req.path.as_str(), req.query);
    let url = Uri::from_str(&url).map_err(IntoRequestError::InvalidUri)?;

    let mut out = hyper::Request::new(req.body.clone().into());

    *out.method_mut() = req.method.clone();
    *out.uri_mut() = url;

    let headers = out.headers_mut();
    copy_headers(req.headers.clone(), headers);

    // Remove the host header as it will be set automatically
    headers.remove("host");

    log::trace!("proxy request (request={:#?})", out);

    Ok(out)
}

#[derive(Debug)]
pub enum ProxyError {
    Hyper(hyper::Error),
    MaxRetries(hyper::Error),
}

impl Reject for ProxyError {}

impl Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} /{}", self.method, self.path.as_str())
    }
}

#[inline]
fn is_h2_goaway_no_error(e: &hyper::Error) -> bool {
    if let Some(source) = e.source() {
        if let Some(e) = source.downcast_ref::<h2::Error>() {
            if e.is_go_away() && e.is_remote() && e.reason() == Some(h2::Reason::NO_ERROR) {
                return true;
            }
        }
    }

    false
}

pub async fn handle<C>(
    http: Client<C>,
    request: Request,
    upstream: &str,
) -> Result<Response, Rejection>
where
    C: Connect + Clone + Send + Sync + 'static,
{
    let mut retries = MAX_RETRIES;

    loop {
        let request = create_proxied_request(&request, upstream)?;

        match http.request(request).await {
            Ok(mut response) => {
                log::trace!("proxy response (response={:#?})", response);

                remove_hop_headers(response.headers_mut());

                return Ok(response);
            }
            Err(e) => {
                // fixes: https://github.com/hyperium/hyper/issues/2500
                if is_h2_goaway_no_error(&e) {
                    retries -= 1;

                    if retries == 0 {
                        return Err(reject::custom(ProxyError::MaxRetries(e)));
                    }

                    log::debug!("retrying request");

                    continue;
                }

                return Err(reject::custom(ProxyError::Hyper(e)));
            }
        }
    }
}
