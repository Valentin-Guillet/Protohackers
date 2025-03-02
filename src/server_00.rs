use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::TcpServer;

pub struct Server {}
impl Server {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl TcpServer for Server {
    async fn handle_connection(&self, mut stream: TcpStream) {
        loop {
            let mut buffer = [0; 1024];
            match stream.read(&mut buffer).await {
                Ok(0) => break,
                Ok(n) => stream.write(&buffer[0..n]).await.unwrap(),
                Err(err) => panic!("{}", err),
            };
        }
    }
}
