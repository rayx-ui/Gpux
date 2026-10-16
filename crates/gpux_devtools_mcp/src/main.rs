//! `gpux-devtools-mcp`: serves MCP on stdio and the gpux apps that connect on loopback.

use gpux_devtools_mcp::{Options, apps::Server, mcp};
use std::io;

fn main() {
    let options = match Options::parse(
        std::env::args().skip(1),
        std::env::var("GPUX_DEVTOOLS_TOKEN").ok(),
    ) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(if message.starts_with("Usage") { 0 } else { 2 });
        }
    };
    if options.print_manifest {
        println!(
            "{}",
            serde_json::to_string_pretty(&mcp::manifest()).expect("the manifest is JSON")
        );
        return;
    }
    let server = match Server::start(options.port, options.token.clone()) {
        Ok(server) => server,
        Err(error) => {
            eprintln!(
                "gpux-devtools-mcp: cannot listen on 127.0.0.1:{}: {error}",
                options.port
            );
            std::process::exit(1);
        }
    };
    eprintln!(
        "gpux-devtools-mcp: waiting for gpux apps on ws://{}",
        server.address
    );
    let stdin = io::stdin();
    if let Err(error) = mcp::serve(&server.registry, stdin.lock(), io::stdout()) {
        eprintln!("gpux-devtools-mcp: {error}");
        std::process::exit(1);
    }
}
