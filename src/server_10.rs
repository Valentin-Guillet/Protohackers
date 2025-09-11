use std::sync::Arc;

use async_trait::async_trait;
use tokio::net::TcpStream;
use tokio::{io::AsyncWriteExt, sync::Mutex};

use crate::{utils, TcpServer};

enum ServerMessage {
    Ok(String),
    Read(String, usize),
    Abort(String),
}

pub fn is_valid_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.contains("//")
        && path
            .chars()
            .all(|c| c.is_alphanumeric() || "/.-_".contains(c))
}

struct Dir {
    name: String,
    subdirs: Vec<Dir>,
    files: Vec<File>,
}

impl Dir {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            subdirs: Vec::new(),
            files: Vec::new(),
        }
    }

    pub fn put_file(&mut self, path: &str, data: String) -> usize {
        let revision;
        if let Some((dir_name, queue)) = path.split_once('/') {
            if let Some(subdir) = self.subdirs.iter_mut().find(|d| d.name == dir_name) {
                revision = subdir.put_file(queue, data);
            } else {
                let mut subdir = Dir::new(dir_name);
                revision = subdir.put_file(queue, data);
                self.subdirs.push(subdir);
            }
        } else {
            if let Some(file) = self.files.iter_mut().find(|f| f.name == path) {
                if data != file.revisions[file.revisions.len() - 1] {
                    file.revisions.push(data);
                }
                revision = file.revisions.len();
            } else {
                let file = File {
                    name: path.to_owned(),
                    revisions: Vec::from([data]),
                };
                revision = file.revisions.len();
                self.files.push(file);
            }
        }
        revision
    }

    pub fn get_file(&self, path: &str) -> Option<&File> {
        if let Some((dir_name, queue)) = path.split_once('/') {
            match self.subdirs.iter().find(|&d| d.name == dir_name) {
                Some(subdir) => subdir.get_file(queue),
                None => None,
            }
        } else {
            self.files.iter().find(|&f| f.name == path)
        }
    }

    pub fn get_list(&self, path: &str) -> Vec<String> {
        if !path.is_empty() {
            let (dir_name, queue) = path.split_once('/').unwrap_or((path, ""));

            return match self.subdirs.iter().find(|&d| d.name == dir_name) {
                Some(subdir) => subdir.get_list(queue),
                None => Vec::new(),
            };
        }

        let file_names: Vec<&str> = self.files.iter().map(|f| f.name.as_str()).collect();
        let mut node_names: Vec<_> = self
            .subdirs
            .iter()
            .filter(|&d| !file_names.contains(&d.name.as_str()))
            .map(|d| format!("{}/ DIR", &d.name))
            .collect();
        node_names.extend(
            self.files
                .iter()
                .map(|f| format!("{} r{}", &f.name, f.revisions.len())),
        );

        node_names.sort_unstable();
        node_names
    }
}

struct File {
    name: String,
    revisions: Vec<String>,
}

struct ServerState {
    root: Dir,
}

impl ServerState {
    pub fn new() -> Self {
        Self { root: Dir::new("") }
    }

    fn get_usage(&self) -> ServerMessage {
        ServerMessage::Ok(String::from("OK usage: HELP|GET|PUT|LIST\nREADY"))
    }

    fn get_file(&mut self, args: &str) -> ServerMessage {
        let args: Vec<&str> = args.split(' ').collect();
        if !(1..=2).contains(&args.len()) {
            return ServerMessage::Ok(String::from("ERR usage: GET file [revision]\nREADY"));
        }

        let path = args[0];
        if !is_valid_path(path) {
            return ServerMessage::Ok(String::from("ERR illegal file name"));
        }

        let Some(file) = self.root.get_file(&path[1..]) else {
            return ServerMessage::Ok(String::from("ERR no such file\nREADY"));
        };

        let revision = match args
            .get(1)
            .map(|&a| a.strip_prefix("r").unwrap_or(a).parse::<usize>())
        {
            Some(Ok(rev)) if (1..=file.revisions.len()).contains(&rev) => rev,
            Some(_) => return ServerMessage::Ok(String::from("ERR no such revision\nREADY")),
            None => file.revisions.len(),
        };

        let data = &file.revisions[revision - 1];
        ServerMessage::Ok(format!("OK {}\n{}READY", data.len(), data))
    }

    fn put_file(&mut self, args: &str) -> ServerMessage {
        let args: Vec<&str> = args.split(' ').collect();
        if args.len() != 2 {
            return ServerMessage::Ok(String::from(
                "ERR usage: PUT file length newline data\nREADY",
            ));
        }

        let path = args[0];
        if !is_valid_path(path) || path.ends_with('/') {
            return ServerMessage::Ok(String::from("ERR illegal file name"));
        }

        let length = args[1].parse::<usize>().unwrap_or_default();
        ServerMessage::Read(path.to_owned(), length)
    }

    fn put_file_data(&mut self, path: String, data: Vec<u8>) -> ServerMessage {
        if !data.iter().all(|c| (0x20..=0x7f).contains(c) || vec![0x09, 0x0a, 0x0d].contains(c)) {
            return ServerMessage::Ok(format!("ERR text files only\nREADY"));
        }
        let data = String::from_utf8(data).unwrap();

        let revision = self.root.put_file(&path[1..], data);
        ServerMessage::Ok(format!("OK r{revision}\nREADY"))
    }

    fn list_files(&mut self, dir_name: &str) -> ServerMessage {
        if dir_name.is_empty() || dir_name.contains(' ') {
            return ServerMessage::Ok(String::from("ERR usage: LIST dir\nREADY"));
        }

        if !is_valid_path(dir_name) {
            return ServerMessage::Ok(String::from("ERR illegal dir name"));
        }

        let mut name_end = dir_name.len();
        if dir_name.ends_with("/") && dir_name.len() > 1 {
            name_end -= 1;
        }

        let list = self.root.get_list(&dir_name[1..name_end]);
        let mut output = format!("OK {}\n", list.len());
        output.push_str(&list.join("\n"));
        if !list.is_empty() {
            output.push('\n');
        }
        output.push_str("READY");
        ServerMessage::Ok(output)
    }

    fn get_response(&mut self, request: &str) -> ServerMessage {
        let (verb, args) = request.split_once(' ').unwrap_or((request, ""));
        let verb = verb.to_uppercase();
        match verb.as_str() {
            "HELP" => self.get_usage(),
            "GET" => self.get_file(args),
            "PUT" => self.put_file(args),
            "LIST" => self.list_files(args),
            _ => ServerMessage::Abort(format!("ERR illegal method: {request}")),
        }
    }
}

pub struct Server {
    state: Arc<Mutex<ServerState>>,
}

impl Server {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ServerState::new())),
        }
    }
}

#[async_trait]
impl TcpServer for Server {
    async fn handle_connection(&self, mut stream: TcpStream) {
        let mut buffer = [0; 1024];

        let _ = stream.write_all("READY\n".as_bytes()).await;
        while let Some(request) = utils::read_until(&mut stream, &mut buffer, '\n').await {
            let mut state = self.state.lock().await;
            let response = state.get_response(&request);
            drop(state);
            match response {
                ServerMessage::Ok(mut msg) => {
                    msg.push('\n');
                    let _ = stream.write_all(msg.as_bytes()).await;
                }
                ServerMessage::Read(path, n) => {
                    let data_len = buffer
                        .iter()
                        .position(|&c| c == b'\0')
                        .unwrap_or(buffer.len());
                    let mut data: Vec<u8>;
                    if data_len < n {
                        let mut buf = Vec::new();
                        let Some(file_data) =
                            utils::read_for(&mut stream, &mut buf, n - data_len).await
                        else {
                            break;
                        };
                        data = Vec::from(&buffer[..data_len]);
                        data.extend(file_data);
                        buffer[..buf.len()].copy_from_slice(&buf[..]);
                        buffer[buf.len()..].fill(0);
                    } else {
                        data = Vec::from(&buffer[..n]);
                        buffer.copy_within(n..data_len, 0);
                        let remaining_len = data_len - n;
                        buffer[remaining_len..].fill(0);
                    }

                    let mut state = self.state.lock().await;
                    match state.put_file_data(path, data) {
                        ServerMessage::Ok(mut msg) => {
                            msg.push('\n');
                            let _ = stream.write_all(msg.as_bytes()).await;
                        }
                        _ => unreachable!(),
                    }
                }
                ServerMessage::Abort(mut msg) => {
                    msg.push('\n');
                    let _ = stream.write_all(msg.as_bytes()).await;
                    break;
                }
            };
        }
    }
}
