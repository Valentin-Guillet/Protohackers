use std::process;

use proto_hackers::get_server;

fn main() {
    let run_server = get_server().unwrap_or_else(|err_msg| {
        println!("Error in argument: {err_msg}");
        process::exit(1);
    });

    run_server();
}
