use std::collections::HashMap;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::utils::{self, read_until};

fn is_valid(name: &str) -> bool {
    name.chars().all(|c| c.is_alphanumeric())
}

struct State {
    connections: HashMap<String, TcpStream>,
}

impl State {
    fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }

    fn add_user(&mut self, username: &str, stream: TcpStream) {
        self.connections.insert(username.to_string(), stream);
    }

    fn remove_user(&mut self, username: &str) {
        self.connections.remove(username);
    }

    fn send_to(&mut self, username: &str, msg: &str) {
        let stream = self.connections.get_mut(username).unwrap();
        stream.write_all(msg.as_bytes()).unwrap();
    }

    fn broadcast_from(&mut self, username: &str, msg: &str) {
        for (name, stream) in self.connections.iter_mut() {
            if name != username {
                stream.write_all(msg.as_bytes()).unwrap();
            }
        }
    }

    fn broadcast_chat(&mut self, from: &str, msg: &str) {
        self.broadcast_from(from, &format!("[{from}] {msg}\n"));
    }
}

fn handle_connection(mut stream: TcpStream, shared_state: Arc<Mutex<State>>) {
    let mut buffer = [0; 1024];
    stream.write_all("Welcome to budgetchat! What shall I call you?\n".as_bytes()).unwrap();

    let username = match read_until(&mut stream, &mut buffer, '\n') {
        None => return,
        Some(name) if !is_valid(&name) => return,
        Some(name) => name,
    };

    let mut state = shared_state.lock().unwrap();
    let mut welcome_msg = String::from("* The room contains: ");
    welcome_msg.push_str(&state.connections.keys().cloned().collect::<Vec<_>>().join(", "));
    welcome_msg.push('\n');

    state.add_user(&username, stream.try_clone().unwrap());
    state.send_to(&username, &welcome_msg);

    let join_msg = format!("* {username} has entered the room\n");
    state.broadcast_from(&username, &join_msg);

    drop(state);  // free the mutex
    while let Some(msg) = utils::read_until(&mut stream, &mut buffer, '\n') {
        let mut state = shared_state.lock().unwrap();
        state.broadcast_chat(&username, &msg);
    }

    let mut state = shared_state.lock().unwrap();
    state.remove_user(&username);
    let exit_msg = format!("* {username} has left the room\n");
    state.broadcast_from(&username, &exit_msg);
}

pub fn run(ip: &str, port: u32) {
    println!("Running server 03");

    let state = Arc::new(Mutex::new(State::new()));

    let listener = TcpListener::bind(format!("{ip}:{port}")).unwrap();

    for stream in listener.incoming() {
        let stream = stream.unwrap();
        println!("Connection established!");
        let state = Arc::clone(&state);
        thread::spawn(move || handle_connection(stream, state));
    }
}
