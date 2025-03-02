use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use tokio::net::UdpSocket;

use crate::UdpServer;

pub struct Server {
    database: Arc<RwLock<HashMap<String, String>>>,
}
impl Server {
    pub fn new() -> Self {
        let database = Arc::new(RwLock::new(HashMap::from([(
            "version".to_string(),
            "Ken's Key-Value Store 1.0".to_string(),
        )])));
        Self { database }
    }

    fn process_request(&self, request: &str) -> Option<String> {
        println!("Processing {request}");
        match request {
            req if req.starts_with("version=") => None,
            req if req.contains("=") => {
                let (key, value) = req.split_once("=").unwrap();
                self.database
                    .write()
                    .unwrap()
                    .insert(key.to_string(), value.to_string());
                None
            }
            req => self
                .database
                .read()
                .unwrap()
                .get(req)
                .map(|value| format!("{}={}", req, value)),
        }
    }
}

#[async_trait]
impl UdpServer for Server {
    async fn handle_connection(&self, socket: &UdpSocket, data: &[u8], addr: &std::net::SocketAddr) {
        let request = String::from_utf8_lossy(data);

        if let Some(response) = self.process_request(&request) {
            socket.send_to(response.as_bytes(), addr).await.unwrap();
        }
    }
}
