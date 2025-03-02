use std::collections::HashMap;
use std::io::Write;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use crate::{utils, TcpServer};

pub struct Server {
    connections: Arc<Mutex<HashMap<String, TcpStream>>>,
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

    fn add_user(&self, username: &str, stream: TcpStream) {
        self.connections
            .lock()
            .unwrap()
            .insert(username.to_string(), stream);
    }

    fn remove_user(&self, username: &str) {
        self.connections.lock().unwrap().remove(username);
    }

    fn send_to(&self, username: &str, msg: &str) {
        let mut connections = self.connections.lock().unwrap();
        let stream = connections.get_mut(username).unwrap();
        stream.write_all(msg.as_bytes()).unwrap();
    }

    fn broadcast_from(&self, username: &str, msg: &str) {
        for (name, stream) in self.connections.lock().unwrap().iter_mut() {
            if name != username {
                stream.write_all(msg.as_bytes()).unwrap();
            }
        }
    }

    fn broadcast_chat(&self, from: &str, msg: &str) {
        self.broadcast_from(from, &format!("[{from}] {msg}\n"));
    }
}

impl TcpServer for Server {
    fn handle_connection(&self, mut stream: TcpStream) {
        let mut buffer = [0; 1024];
        stream
            .write_all("Welcome to budgetchat! What shall I call you?\n".as_bytes())
            .unwrap();

        let username = match utils::read_until(&mut stream, &mut buffer, '\n') {
            None => return,
            Some(name) if !Self::is_valid(&name) => return,
            Some(name) => name,
        };

        let mut welcome_msg = String::from("* The room contains: ");
        let connections = self.connections.lock().unwrap();
        welcome_msg.push_str(&connections.keys().cloned().collect::<Vec<_>>().join(", "));
        welcome_msg.push('\n');
        drop(connections);

        self.add_user(&username, stream.try_clone().unwrap());
        self.send_to(&username, &welcome_msg);

        let join_msg = format!("* {username} has entered the room\n");
        self.broadcast_from(&username, &join_msg);

        while let Some(msg) = utils::read_until(&mut stream, &mut buffer, '\n') {
            self.broadcast_chat(&username, &msg);
        }

        self.remove_user(&username);
        let exit_msg = format!("* {username} has left the room\n");
        self.broadcast_from(&username, &exit_msg);
    }
}
