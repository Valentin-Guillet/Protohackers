use async_trait::async_trait;
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::{utils, TcpServer};

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

pub struct Server {}
impl Server {
    pub fn new() -> Self {
        Self {}
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
}

#[async_trait]
impl TcpServer for Server {
    async fn handle_connection(&self, mut stream: TcpStream) {
        let mut buffer = [0; 1024];
        while let Some(request) = utils::read_until(&mut stream, &mut buffer, '\n').await {
            let response = Self::get_response(&request).unwrap_or(String::from("{}\n"));
            println!("Request {} -> response {}", request.trim(), response.trim());
            if stream.write_all(response.as_bytes()).await.is_err() {
                break;
            }
        }
    }
}
