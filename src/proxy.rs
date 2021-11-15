#![deny(warnings)]

use std::io::Write;
use lazy_static::lazy_static;
use std::net::IpAddr;
use std::str::FromStr;
use futures::StreamExt;
use mime::Mime;
use unicase::Ascii;
use http::header;
use hyper_tls::HttpsConnector;
use hyper::header::{HeaderMap, HeaderValue};
use hyper::http::header::{InvalidHeaderValue, ToStrError};
use hyper::http::uri::InvalidUri;
use hyper::{Body, Client, Error, Request, Response, Uri};
use hyper::body::Buf;
use url::{ParseError};
use brotli;

// TODO: maybe in the future
const ENCODING_SUPPORT: bool = false;

const HOP_HEADER_CONNECTION: &str = "Connection";
const HOP_HEADER_PROXY_CONNECTION: &str = "Proxy-Connection";
const HOP_HEADER_KEEP_ALIVE: &str = "Keep-Alive";
const HOP_HEADER_PROXY_AUTHENTICATE: &str = "Proxy-Authenticate";
const HOP_HEADER_PROXY_AUTHORIZATION: &str = "Proxy-Authorization";
const HOP_HEADER_TE: &str = "Te";
const HOP_HEADER_TRAILER: &str = "Trailer";
const HOP_HEADER_TRANSFER_ENCODING: &str = "Transfer-Encoding";
const HOP_HEADER_UPGRADE: &str = "Upgrade";

const HOP_HEADER_FORWARDED_FOR: &str = "x-forwarded-for";

pub enum ProxyError {
    InvalidUri(InvalidUri),
    HyperError(Error),
    ForwardHeaderError,
    ParseURLError(ParseError),
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

impl From<ParseError> for ProxyError {
    fn from(err: ParseError) -> Self {
        ProxyError::ParseURLError(err)
    }
}

impl From<std::str::Utf8Error> for ProxyError {
    fn from(_: std::str::Utf8Error) -> Self {
        ProxyError::ForwardHeaderError
    }
}

fn is_hop_header(name: &str) -> bool {
    lazy_static! {
        static ref HOP_HEADERS: Vec<Ascii<&'static str>> = vec![
            Ascii::new(HOP_HEADER_CONNECTION),
            Ascii::new(HOP_HEADER_PROXY_CONNECTION),
            Ascii::new(HOP_HEADER_KEEP_ALIVE),
            Ascii::new(HOP_HEADER_PROXY_AUTHENTICATE),
            Ascii::new(HOP_HEADER_PROXY_AUTHORIZATION),
            Ascii::new(HOP_HEADER_TE),
            Ascii::new(HOP_HEADER_TRAILER),
            Ascii::new(HOP_HEADER_TRANSFER_ENCODING),
            Ascii::new(HOP_HEADER_UPGRADE),
        ];
    }

    HOP_HEADERS.iter().any(|h| h == &name)
}

fn do_not_forward_request_headers(name: &str) -> bool {
    return match name {
        "host" => true,
        _ => do_not_forward_encoding(name)
    };
}

// TODO: It's workaround for ignoring website encoding like brotli. It's cumbersome for injecting JS into encoded website.
fn do_not_forward_encoding(name: &str) -> bool {
    return match name {
        "content-encoding" => true,
        "accept-encoding" => true,
        _ => false
    };
}

pub struct Proxy {}

impl Proxy {
    fn remove_hop_headers(headers: &HeaderMap<HeaderValue>, request: bool) -> HeaderMap<HeaderValue> {
        let mut result = HeaderMap::new();

        for (k, v) in headers.iter() {
            let name = k.as_str();

            if is_hop_header(name) || (request && do_not_forward_request_headers(name)) {
                continue;
            }

            result.insert(k.clone(), v.clone());
        }

        result
    }

    async fn create_proxied_response(inject_js: String, mut response: Response<Body>) -> Result<Response<Body>, ProxyError> {
        match response.headers_mut().entry(header::CONTENT_TYPE) {
            hyper::header::Entry::Occupied(entry) => {
                let val = entry.get().to_str().unwrap();
                let mime: Mime = val.parse().unwrap();

                match (mime.type_(), mime.subtype()) {
                    (mime::TEXT, mime::HTML) => {
                        if inject_js.is_empty() {
                            return Ok(response);
                        }
                    }
                    _ => {
                        return Ok(response);
                    }
                }
            }
            _ => {}
        }


        let mut chunks = vec![];

        while let Some(chunk) = response.body_mut().next().await {
            chunks.extend_from_slice(chunk?.chunk());
        }

        // TODO: html encoding - brotli etc.
        if ENCODING_SUPPORT {
            match response.headers_mut().entry(header::CONTENT_ENCODING) {
                hyper::header::Entry::Occupied(entry) => {
                    let val = entry.get().to_str().unwrap();

                    let script = format!("{}{}{}", "<script>", inject_js, "</script>");

                    match val {
                        "br" => { // todo: brotli needs bitstream concatenation
                            let mut vec = vec![];

                            {
                                let mut writer = brotli::CompressorWriter::new(&mut vec, 4096, 0, 20);
                                writer.write(script.as_bytes()).unwrap();
                            }
                            {
                                chunks.extend_from_slice(&mut vec); // TODO: this not work because we cant concat brotli like that
                            }
                        }
                        _ => {
                            chunks.extend_from_slice(script.as_bytes());
                        }
                    }
                }
                _ => {}
            }
        } else {
            let script = format!("{}{}{}", "<script>", inject_js, "</script>");
            chunks.extend_from_slice(script.as_bytes());
        }

        let content_length = chunks.iter().len().to_string();

        *response.body_mut() = Body::from(chunks);
        *response.headers_mut() = Proxy::remove_hop_headers(response.headers(), false);
        response.headers_mut().insert(header::CONTENT_LENGTH, content_length.parse().unwrap());

        Ok(response)
    }

    fn create_proxied_request<B>(
        client_ip: IpAddr,
        forward_url: String,
        mut request: Request<B>,
    ) -> Result<Request<B>, ProxyError> {
        *request.headers_mut() = Proxy::remove_hop_headers(request.headers(), true);
        *request.uri_mut() = Proxy::forward_uri(forward_url, &request)?;

        match request.headers_mut().entry(HOP_HEADER_FORWARDED_FOR) {
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

    fn forward_uri<B>(forward_url: String, req: &Request<B>) -> Result<Uri, InvalidUri> {
        let forward_uri = match req.uri().query() {
            Some(query) => format!("{}{}?{}", forward_url, req.uri().path(), query),
            None => format!("{}{}", forward_url, req.uri().path()),
        };

        Uri::from_str(forward_uri.as_str())
    }

    // TODO: If https then use default or from argument TLS certs. Otherwise dont use TLS.
    // TODO: issues with redirections - should request to location if redirected
    pub async fn call(
        client_ip: IpAddr,
        forward_url: String,
        inject_js: String,
        request: Request<Body>,
    ) -> Result<Response<Body>, ProxyError> {
        let proxied_request = Proxy::create_proxied_request(client_ip, forward_url, request)?;

        // TODO: htp connector
        // let client: Client<HttpConnector>;
        // let client = Client::new();

        let https = HttpsConnector::new();
        let client = Client::builder().build::<_, hyper::Body>(https);

        let response = client.request(proxied_request).await?;
        let proxied_response = Proxy::create_proxied_response(inject_js, response).await?;

        Ok(proxied_response)
    }
}

