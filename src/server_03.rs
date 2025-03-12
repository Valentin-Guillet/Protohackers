use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, tcp::OwnedWriteHalf};
use tokio::sync::Mutex;

use crate::{TcpServer, utils};

pub struct Server {
    connections: Arc<Mutex<HashMap<String, OwnedWriteHalf>>>,
}

impl Server {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn is_valid(name: &str) -> bool {
        name.chars().all(|c| c.is_alphanumeric())
    }

    async fn add_user(&self, username: &str, writer: OwnedWriteHalf) {
        self.connections
            .lock()
            .await
            .insert(username.to_string(), writer);
    }

    async fn remove_user(&self, username: &str) {
        self.connections.lock().await.remove(username);
    }

    async fn send_to(&self, username: &str, msg: &str) {
        let mut connections = self.connections.lock().await;
        let writer = connections.get_mut(username).unwrap();
        writer.write_all(msg.as_bytes()).await.unwrap();
    }

    async fn broadcast_from(&self, username: &str, msg: &str) {
        for (name, writer) in self.connections.lock().await.iter_mut() {
            if name != username {
                writer.write_all(msg.as_bytes()).await.unwrap();
            }
        }
    }

    async fn broadcast_chat(&self, from: &str, msg: &str) {
        self.broadcast_from(from, &format!("[{from}] {msg}\n"))
            .await;
    }
}

#[async_trait]
impl TcpServer for Server {
    async fn handle_connection(&self, stream: TcpStream) {
        let mut buffer = [0; 1024];

        let (mut reader, mut writer) = stream.into_split();
        writer
            .write_all("Welcome to budgetchat! What shall I call you?\n".as_bytes())
            .await
            .unwrap();

        let username = match utils::read_until(&mut reader, &mut buffer, '\n').await {
            None => return,
            Some(name) if !Self::is_valid(&name) => return,
            Some(name) => name,
        };

        let connections = self.connections.lock().await;
        let welcome_msg = format!(
            "* The room contains {}\n",
            connections.keys().cloned().collect::<Vec<_>>().join(", ")
        );
        drop(connections);

        self.add_user(&username, writer).await;
        self.send_to(&username, &welcome_msg).await;

        let join_msg = format!("* {username} has entered the room\n");
        self.broadcast_from(&username, &join_msg).await;

        while let Some(msg) = utils::read_until(&mut reader, &mut buffer, '\n').await {
            self.broadcast_chat(&username, &msg).await;
        }

        self.remove_user(&username).await;
        let exit_msg = format!("* {username} has left the room\n");
        self.broadcast_from(&username, &exit_msg).await;
    }
}
