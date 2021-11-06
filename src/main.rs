#![deny(warnings)]

mod server;
mod proxy;

use clap::{App, Arg};

#[tokio::main]
async fn main() -> Result<(), String> {
    let app = App::new("nemo-proxy")
        .version("0.1")
        .author("zdunekhere@gmail.com")
        .about("Nemo is a pluggable phishing proxy with JavaScript injection written in Rust")
        .arg(
            Arg::with_name("address")
                .short("a")
                .default_value("127.0.0.1:8000")
        )
        .arg(
            Arg::with_name("url")
                // .required(true)
                .short("u")
                .default_value("https://livesession.io")
        );

    let matches = app.get_matches();
    let cmd_bind_addr = matches.value_of("address").ok_or_else(|| "address is required")?;
    let cmd_forward_url = matches.value_of("url").ok_or_else(|| "url is required")?;

    server::run(cmd_bind_addr, cmd_forward_url).await;

    Ok(())
}