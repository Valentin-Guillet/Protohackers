use std::process;

use proto_hackers::{Server, get_challenge, get_ip};

#[tokio::main]
async fn main() {
    let server = get_challenge()
        .and_then(Server::new)
        .unwrap_or_else(|err_msg| {
            println!("Error in argument: {err_msg}");
            process::exit(1);
        });

    let ip = get_ip().expect("Could not get IP address");
    let port = 12233;
    server.run(&ip, port).await;
}
