use std::{
    borrow::Cow,
    fmt::{self, Display},
    io::{self, ErrorKind},
    net::IpAddr,
};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use reqwest::{
    header::{HeaderName, HeaderValue},
    Body, Client, Url,
};
use rocket::{
    data::ToByteUnit,
    http::{Header, HeaderMap, Method, Status},
    request::{FromRequest, Outcome},
    response::{status, stream::stream},
    Data,
};

use super::stream::ByteStreamResponse;

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

fn copy_headers(headers: reqwest::header::HeaderMap, dest: &mut HeaderMap) {
    let mut header_name = None;

    for (name, value) in headers {
        let name = name.unwrap_or_else(|| header_name.clone().unwrap());

        if is_hop_header(name.as_str()) {
            continue;
        }

        // @TODO: remove string allocations
        dest.add(Header::new(
            name.to_string(),
            value.to_str().unwrap().to_string(),
        ));

        header_name = Some(name);
    }
}

pub struct Request<'r> {
    method: Method,
    path: Cow<'r, str>,
    headers: Option<HeaderMap<'r>>,
    client_ip: Option<IpAddr>,
}

impl<'r> Request<'r> {
    pub fn new(method: Method, path: impl Into<Cow<'r, str>>) -> Self {
        Self {
            method,
            path: path.into(),
            headers: None,
            client_ip: None,
        }
    }

    #[inline]
    pub fn set_path(&mut self, path: impl Into<Cow<'r, str>>) {
        self.path = path.into();
    }

    pub fn map(self, upstream: &str) -> reqwest::Request {
        let url = [upstream, &self.path].concat();
        let mut request = reqwest::Request::new(
            self.method.as_str().parse().unwrap(),
            Url::parse(&url).unwrap(),
        );

        let request_headers = request.headers_mut();

        if let Some(headers) = self.headers {
            for header in headers.into_iter() {
                let name = header.name.as_str();

                if is_hop_header(name) {
                    continue;
                }

                // Using `unwrap` should be safe as the headers were succesfully parsed in the first place
                request_headers.insert(
                    name.parse::<HeaderName>().unwrap(),
                    HeaderValue::from_str(header.value.as_ref()).unwrap(),
                );
            }
        }

        request
    }
}

impl<'r> Display for Request<'r> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} /{}", self.method, self.path)
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Request<'r> {
    type Error = anyhow::Error;

    async fn from_request(req: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
        let mut request = Request::new(req.method(), req.uri().path().as_str());
        request.headers = Some(req.headers().clone());

        if let Outcome::Success(ip_addr) = IpAddr::from_request(req).await {
            request.client_ip = Some(ip_addr);
        }

        Outcome::Success(request)
    }
}

pub async fn handle<'r>(
    http: Client,
    request: Request<'r>,
    upstream: &str,
    body: Data<'r>,
) -> Result<
    ByteStreamResponse<'r, impl Stream<Item = Result<Bytes, io::Error>>>,
    status::Custom<String>,
> {
    let body = body.open(15.mebibytes()).into_bytes().await.map_err(|e| {
        status::Custom(
            Status::InternalServerError,
            format!("failed to read body: {}", e),
        )
    })?;

    let mut request = request.map(upstream);
    request.body_mut().replace(Body::from(body.value));

    let response = http.execute(request).await.map_err(|e| {
        status::Custom(
            Status::InternalServerError,
            format!("request failed: {}", e),
        )
    })?;
    let status = response.status().as_u16();
    let headers = response.headers().clone();

    let bytes_response = ByteStreamResponse::new(stream! {
        let mut response_stream = response.bytes_stream();

        while let Some(item) = response_stream.next().await {
            yield item.map_err(|e| io::Error::new(ErrorKind::Other, e));
        }
    });

    let mut headers_map = HeaderMap::new();
    copy_headers(headers, &mut headers_map);

    Ok(bytes_response
        .set_headers(headers_map)
        .set_status(Status::from_code(status).unwrap()))
}
