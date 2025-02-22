use std::{
    io::{Read, Write},
    net::TcpStream,
};

use serde_json::json;

fn is_prime(n: f64) -> bool {
    if n.fract() != 0.0 || n < 2.0 {
        return false;
    }

    let n = n as u64;
    if n == 2 || n == 3 || n == 5 {
        return true;
    }

    if n % 2 == 0 || n % 3 == 0 || n % 5 == 0 {
        return false;
    }

    let limit = (n as f64).sqrt().abs() as u64 + 1;
    let mut k = 1;
    while 6 * k < limit {
        if n % (6 * k + 1) == 0 || n % (6 * k + 5) == 0 {
            return false;
        }
        k += 1;
    }
    true
}

fn get_response(buf: &str) -> Option<String> {
    let object: serde_json::Value = serde_json::from_str(buf).ok()?;
    let object = object.as_object()?;

    let method = object.get("method")?.as_str()?;
    let number = object.get("number")?.as_f64()?;

    if method != "isPrime" {
        return None;
    }

    let response = json!({
        "method": "isPrime",
        "prime": is_prime(number)
    });

    Some(response.to_string() + "\n")
}

pub fn run(mut stream: TcpStream) {
    let mut requests = String::new();
    loop {
        let mut read = [0; 4096];
        match stream.read(&mut read) {
            Err(err) => panic!("{}", err),
            Ok(0) => break,
            Ok(n) => {
                requests.push_str(&String::from_utf8_lossy(&read[0..n]));
                while requests.contains("\n") {
                    let request: String =
                        requests.drain(..requests.find("\n").unwrap() + 1).collect();
                    let response = get_response(&request).unwrap_or(String::from("{}\n"));
                    println!("Request {} -> response {}", request.trim(), response.trim());
                    if stream.write_all(response.as_bytes()).is_err() {
                        break;
                    }
                }
            }
        }
    }
}
