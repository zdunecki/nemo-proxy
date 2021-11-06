#![deny(warnings)]

use crate::proxy;
use hyper::server::conn::AddrStream;
use hyper::{Body, Request, Response, Server, StatusCode};
use hyper::service::{service_fn, make_service_fn};
use std::{convert::Infallible, net::SocketAddr};

use std::net::IpAddr;

async fn handle(client_ip: IpAddr, forward_uri: String, req: Request<Body>) -> Result<Response<Body>, Infallible> {
    match proxy::call(client_ip, forward_uri, req).await {
        Ok(response) => {
            Ok(response)
        }
        Err(_err) => {
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap())
        }
    }
}

pub async fn run(bind_addr: &str, forward_url: &str) {
    let addr: SocketAddr = bind_addr.parse().expect("Could not parse ip:port.");

    let make_svc = make_service_fn(|conn: &AddrStream| {
        let remote_addr = conn.remote_addr().ip();
        let _ip = remote_addr.to_string();
        let forward_url = String::from(forward_url); // to avoid borrows errors with static lifetime move their ownership

        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                handle(remote_addr, forward_url.clone(), req) // clone to avoid move errors
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }

    println!("Running server on {:?}", addr);
}