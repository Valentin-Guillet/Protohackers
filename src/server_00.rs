use std::io::{Read, Write};
use std::net::TcpStream;

use crate::TcpServer;

pub struct Server {}
impl Server {
    pub fn new() -> Self {
        Self {}
    }
}

impl TcpServer for Server {
    fn handle_connection(&self, mut stream: TcpStream) {
        loop {
            let mut buffer = [0; 1024];
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => stream.write_all(&buffer[0..n]).unwrap(),
                Err(err) => panic!("{}", err),
            }
        }
    }
}
