use std::{
    error::Error,
    fmt::{self, Display},
};

use bytes::Bytes;
use http::{header::HeaderName, uri::InvalidUri, HeaderMap, Method, Uri};
use hyper::{client::connect::Connect, Body, Client};
use warp::{
    path::Tail,
    reject::{self, Reject},
    reply::Response,
    Rejection,
};

const MAX_RETRIES: usize = 3;

static HOP_HEADERS: [HeaderName; 8] = [
    HeaderName::from_static("connection"),
    HeaderName::from_static("keep-alive"),
    HeaderName::from_static("proxy-authenticate"),
    HeaderName::from_static("proxy-authorization"),
    HeaderName::from_static("te"),
    HeaderName::from_static("trailers"),
    HeaderName::from_static("transfer-encoding"),
    HeaderName::from_static("upgrade"),
];

#[inline(always)]
fn remove_hop_headers(headers: &mut HeaderMap) {
    for header in HOP_HEADERS.iter() {
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

fn build_uri(req: &Request, upstream: &str) -> Result<Uri, InvalidUri> {
    let request_path = req.path.as_str();
    let mut url = String::with_capacity(
        upstream.len()
            + request_path.len()
            + if req.query.is_empty() {
                0
            } else {
                req.query.len() + 1
            },
    );

    url.push_str(upstream);
    url.push_str(request_path);

    if !req.query.is_empty() {
        url.push('?');
        url.push_str(&req.query);
    }

    Uri::try_from(url)
}

fn prepare_headers(req: &Request) -> HeaderMap {
    let mut headers = HeaderMap::new();
    copy_headers(req.headers.clone(), &mut headers);

    // Remove the host header as it will be set automatically
    headers.remove("host");
    headers
}

fn create_proxied_request(
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<hyper::Request<Body>, Rejection> {
    let mut out = hyper::Request::new(body.into());

    *out.headers_mut() = headers;
    *out.method_mut() = method;
    *out.uri_mut() = uri;

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

    // Prepare the request once to avoid doing more work in case of a retry
    let uri = build_uri(&request, upstream).map_err(IntoRequestError::InvalidUri)?;
    let headers = prepare_headers(&request);
    let (method, body) = (request.method, request.body);

    loop {
        let request =
            create_proxied_request(method.clone(), uri.clone(), headers.clone(), body.clone())?;

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
