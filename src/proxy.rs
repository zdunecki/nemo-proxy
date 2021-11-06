#![deny(warnings)]

use hyper::header::{HeaderMap, HeaderValue};
use hyper::http::header::{InvalidHeaderValue, ToStrError};
use hyper::http::uri::InvalidUri;
use hyper::{Body, Client, Error, Request, Response, Uri};
use lazy_static::lazy_static;
use std::net::IpAddr;
use std::str::FromStr;
use unicase::Ascii;
use hyper_tls::HttpsConnector;

const HEADER_CONNECTION: &str = "Connection";
const HEADER_KEEP_ALIVE: &str = "Keep-Alive";
const HEADER_PROXY_AUTHENTICATE: &str = "Proxy-Authenticate";
const HEADER_PROXY_AUTHORIZATION: &str = "Proxy-Authorization";
const HEADER_TE: &str = "Te";
const HEADER_TRAILERS: &str = "Trailers";
const HEADER_TRANSFER_ENCODING: &str = "Transfer-Encoding";
const HEADER_UPGRADE: &str = "Upgrade";

const HEADER_FORWARDED_FOR: &str = "x-forwarded-for";

pub enum ProxyError {
    InvalidUri(InvalidUri),
    HyperError(Error),
    ForwardHeaderError,
}

impl From<Error> for ProxyError {
    fn from(err: Error) -> ProxyError {
        ProxyError::HyperError(err)
    }
}

impl From<InvalidUri> for ProxyError {
    fn from(err: InvalidUri) -> ProxyError {
        ProxyError::InvalidUri(err)
    }
}

impl From<ToStrError> for ProxyError {
    fn from(_err: ToStrError) -> ProxyError {
        ProxyError::ForwardHeaderError
    }
}

impl From<InvalidHeaderValue> for ProxyError {
    fn from(_err: InvalidHeaderValue) -> ProxyError {
        ProxyError::ForwardHeaderError
    }
}

fn is_hop_header(name: &str) -> bool {
    lazy_static! {
        static ref HOP_HEADERS: Vec<Ascii<&'static str>> = vec![
            Ascii::new(HEADER_CONNECTION),
            Ascii::new(HEADER_KEEP_ALIVE),
            Ascii::new(HEADER_PROXY_AUTHENTICATE),
            Ascii::new(HEADER_PROXY_AUTHORIZATION),
            Ascii::new(HEADER_TE),
            Ascii::new(HEADER_TRAILERS),
            Ascii::new(HEADER_TRANSFER_ENCODING),
            Ascii::new(HEADER_UPGRADE),
        ];
    }

    HOP_HEADERS.iter().any(|h| h == &name);

    true // TODO: for some reasons https + above hop headers not working
}

fn remove_hop_headers(headers: &HeaderMap<HeaderValue>) -> HeaderMap<HeaderValue> {
    let mut result = HeaderMap::new();

    for (k, v) in headers.iter() {
        if !is_hop_header(k.as_str()) {
            result.insert(k.clone(), v.clone()); // TODO: find save headers to proxy
        }
    }
    result
}

fn create_proxied_response<B>(mut response: Response<B>) -> Response<B> {
    *response.headers_mut() = remove_hop_headers(response.headers());
    response
}

fn forward_uri<B>(forward_url: String, req: &Request<B>) -> Result<Uri, InvalidUri> {
    let forward_uri = match req.uri().query() {
        Some(query) => format!("{}{}?{}", forward_url, req.uri().path(), query),
        None => format!("{}{}", forward_url, req.uri().path()),
    };

    Uri::from_str(forward_uri.as_str())
}

fn create_proxied_request<B>(
    client_ip: IpAddr,
    forward_url: String,
    mut request: Request<B>,
) -> Result<Request<B>, ProxyError> {
    *request.headers_mut() = remove_hop_headers(request.headers());
    *request.uri_mut() = forward_uri(forward_url, &request)?;

    match request.headers_mut().entry(HEADER_FORWARDED_FOR) {
        hyper::header::Entry::Vacant(entry) => {
            entry.insert(client_ip.to_string().parse()?);
        }

        hyper::header::Entry::Occupied(mut entry) => {
            let addr = format!("{}, {}", entry.get().to_str()?, client_ip);
            entry.insert(addr.parse()?);
        }
    }

    Ok(request)
}

//TODO: If https then use default or from argument TLS certs. Otherwise dont use TLS.
//TODO: fix issues with https

pub async fn call(
    client_ip: IpAddr,
    forward_uri: String,
    request: Request<Body>,
) -> Result<Response<Body>, ProxyError> {
    let proxied_request = create_proxied_request(client_ip, forward_uri, request)?;

    // let client: Client<HttpConnector>;

    let https = HttpsConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    // let client = Client::new();
    let response = client.request(proxied_request).await?;
    let proxied_response = create_proxied_response(response);
    Ok(proxied_response)
}