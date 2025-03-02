use std::collections::HashMap;
use std::net::UdpSocket;

fn process_request(database: &mut HashMap<String, String>, request: &str) -> Option<String> {
    println!("Processing {request}");
    match request {
        req if req.starts_with("version=") => None,
        req if req.contains("=") => {
            let (key, value) = req.split_once("=").unwrap();
            database.insert(key.to_string(), value.to_string());
            None
        }
        req => database.get(req).map(|value| format!("{}={}", req, value)),
    }
}

pub fn run(ip: &str, port: u32) {
    println!("Running server 04 on {ip} / {port}");
    let socket = UdpSocket::bind(format!("{ip}:{port}")).unwrap();

    let mut database: HashMap<String, String> = HashMap::new();
    database.insert(
        "version".to_string(),
        "Ken's Key-Value Store 1.0".to_string(),
    );
    loop {
        let mut buffer = [0; 1024];
        match socket.recv_from(&mut buffer) {
            Err(_) => break,
            Ok((n, addr)) => {
                let response =
                    process_request(&mut database, &String::from_utf8_lossy(&buffer[..n]));
                if let Some(response) = response {
                    socket.send_to(response.as_bytes(), addr).unwrap();
                };
            }
        };
    }
}
