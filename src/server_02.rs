use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::{TcpServer, utils};

pub struct Server {}
impl Server {
    pub fn new() -> Self {
        Self {}
    }

    fn get_response(data: &mut Vec<(i32, i32)>, buf: &[u8]) -> Option<i32> {
        let query_type: char = char::from(buf[0]);
        let first = i32::from_be_bytes(buf[1..5].try_into().unwrap());
        let second = i32::from_be_bytes(buf[5..].try_into().unwrap());

        match query_type {
            'Q' => {
                let (sum, count) = data
                    .iter()
                    .filter(|(timestamp, _)| (first..=second).contains(timestamp))
                    .fold((0, 0), |(sum, count), &(_, price)| {
                        (sum + (price as i64), count + 1)
                    });
                if count > 0 {
                    Some((sum / count) as i32)
                } else {
                    Some(0)
                }
            }
            'I' => {
                data.push((first, second));
                None
            }
            _ => None,
        }
    }
}

#[async_trait]
impl TcpServer for Server {
    async fn handle_connection(&self, mut stream: TcpStream) {
        let mut data = Vec::new();
        let mut buffer: Vec<u8> = Vec::new();
        while let Some(request) = utils::read_for(&mut stream, &mut buffer, 9).await {
            println!("Request: {:?}", request);
            let response = Self::get_response(&mut data, &request);
            if response.is_some()
                && stream
                    .write_all(&response.unwrap().to_be_bytes())
                    .await
                    .is_err()
            {
                break;
            }
        }
    }
}
