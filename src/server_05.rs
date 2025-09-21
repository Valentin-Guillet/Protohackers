use std::sync::LazyLock;

use async_trait::async_trait;
use fancy_regex::Regex;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::{TcpServer, utils};

static BOGUSCOIN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?<=^| )7[[:alnum:]]{25,34}(?=$| )").unwrap());

pub struct Server {}
impl Server {
    pub fn new() -> Self {
        Self {}
    }

    fn poison_msg(msg: String) -> String {
        BOGUSCOIN_RE
            .replace_all(&msg, "7YWHMfk9JZe0LM0g1ZauHuiSxhI")
            .into()
    }

    async fn connect_streams(reader: &mut OwnedReadHalf, writer: &mut OwnedWriteHalf) {
        let mut buffer = [0; 1024];
        while let Some(msg) = utils::read_until(reader, &mut buffer, '\n').await {
            let poisoned_msg = Self::poison_msg(msg) + "\n";
            let _ = writer.write_all(poisoned_msg.as_bytes()).await;
        }
    }
}

#[async_trait]
impl TcpServer for Server {
    async fn handle_connection(&self, stream: TcpStream) {
        let server_stream = TcpStream::connect("chat.protohackers.com:16963")
            .await
            .unwrap();
        let (mut client_reader, mut client_writer) = stream.into_split();
        let (mut server_reader, mut server_writer) = server_stream.into_split();

        let thread_1 = tokio::spawn(async move {
            Self::connect_streams(&mut client_reader, &mut server_writer).await;
        });

        let thread_2 = tokio::spawn(async move {
            Self::connect_streams(&mut server_reader, &mut client_writer).await;
        });

        let _ = thread_1.await.unwrap();
        let _ = thread_2.await.unwrap();
    }
}
