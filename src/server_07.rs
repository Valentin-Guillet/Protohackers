use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use fancy_regex::Regex;
use tokio::{net::UdpSocket, sync::Mutex, task::JoinHandle, time};

use crate::UdpServer;

#[derive(Clone)]
enum ServerMessage {
    Ack { session_id: u32 },
    Data { session_id: u32, data: String },
    Close { session_id: u32 },
}

struct Session {
    data_received: String,
    data_to_send: String,
    length_received: usize,
    length_sent: usize,
    length_acked: usize,
}
impl Session {
    fn new() -> Self {
        Self {
            data_received: String::new(),
            data_to_send: String::new(),
            length_received: 0,
            length_sent: 0,
            length_acked: 0,
        }
    }

    fn push(&mut self, data: &str) {
        self.length_received += data.len();
        if !data.contains('\n') {
            self.data_received.push_str(data);
            return;
        }

        let lines = data.split("\n").collect::<Vec<_>>();
        self.data_to_send.extend(lines[0].chars().rev());
        self.data_to_send.extend(self.data_received.chars().rev());
        self.data_to_send.push('\n');
        self.data_received.clear();

        for line in &lines[1..lines.len() - 1] {
            self.data_to_send.extend(line.chars().rev());
            self.data_to_send.push('\n');
        }
        self.data_received = lines[lines.len() - 1].to_string();
    }

    fn get_message(&mut self) -> Option<String> {
        if self.data_to_send.is_empty() {
            return None;
        }

        let len = self.data_to_send.len().min(950);
        Some(self.data_to_send[..len].to_string())
    }

    fn acknowledge(&mut self) {
        let _ = self
            .data_to_send
            .drain(..self.length_sent - self.length_acked);
        self.length_acked = self.length_sent;
    }
}

struct ServerState {
    sessions: HashMap<u32, Session>,
    regexes: HashMap<&'static str, Regex>,
}
impl ServerState {
    fn escape(data: &str) -> String {
        data.replace(r"\", r"\\").replace(r"/", r"\/")
    }

    fn unescape(data: &str) -> String {
        data.replace(r"\\", r"\").replace(r"\/", r"/")
    }

    fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            regexes: HashMap::from([
                (
                    "connect",
                    Regex::new(r"^/connect/(?<session_id>\d+)/$").unwrap(),
                ),
                (
                    "data",
                    Regex::new(
                        r"^/data/(?<session_id>\d+)/(?<pos>\d+)/(?<data>(?:[^/\\]|\\/|\\\\)+)/$",
                    )
                    .unwrap(),
                ),
                (
                    "ack",
                    Regex::new(r"^/ack/(?<session_id>\d+)/(?<pos>\d+)/$").unwrap(),
                ),
                (
                    "close",
                    Regex::new(r"^/close/(?<session_id>\d+)/$").unwrap(),
                ),
            ]),
        }
    }

    fn connect_session(&mut self, session_id: u32) -> Result<Vec<ServerMessage>> {
        self.sessions.entry(session_id).or_insert(Session::new());
        Ok(vec![ServerMessage::Data {
            session_id,
            data: format!("/ack/{session_id}/0/"),
        }])
    }

    fn receive_data(
        &mut self,
        session_id: u32,
        pos: usize,
        data: &str,
    ) -> Result<Vec<ServerMessage>> {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Ok(vec![ServerMessage::Close { session_id }]);
        };

        let data = Self::unescape(data);
        let mut responses = Vec::new();

        if session.length_received == pos {
            responses.push(ServerMessage::Data {
                session_id,
                data: format!("/ack/{session_id}/{}/", pos + data.len()),
            });
            session.push(&data);

            // Only send message if we're uptodate on data to send
            if session.length_acked == session.length_sent {
                if let Some(response) = session.get_message() {
                    responses.push(ServerMessage::Data {
                        session_id,
                        data: format!(
                            "/data/{}/{}/{}/",
                            session_id,
                            session.length_acked,
                            Self::escape(&response)
                        ),
                    });
                    session.length_sent += response.len();
                }
            }
        } else {
            responses.push(ServerMessage::Data {
                session_id,
                data: format!("/ack/{session_id}/{}/", session.length_received),
            });
        }

        Ok(responses)
    }

    fn receive_ack(&mut self, session_id: u32, pos: usize) -> Result<Vec<ServerMessage>> {
        let Some(session) = self.sessions.get_mut(&session_id) else {
            return Ok(vec![ServerMessage::Close { session_id }]);
        };

        let mut responses = vec![ServerMessage::Ack { session_id }];
        if pos < session.length_acked {
            Ok(responses)
        } else if pos == session.length_sent {
            session.acknowledge();
            if let Some(response) = session.get_message() {
                responses.push(ServerMessage::Data {
                    session_id,
                    data: format!(
                        "/data/{}/{}/{}/",
                        session_id,
                        session.length_acked,
                        Self::escape(&response)
                    ),
                });
                session.length_sent += response.len();
            }
            Ok(responses)
        } else if pos > session.length_sent {
            responses.push(ServerMessage::Close { session_id });
            Ok(responses)
        } else {
            let response = session.get_message().unwrap();
            responses.push(ServerMessage::Data {
                session_id,
                data: format!(
                    "/data/{}/{}/{}/",
                    session_id,
                    session.length_acked,
                    Self::escape(&response)
                ),
            });
            Ok(responses)
        }
    }

    fn close_session(&mut self, session_id: u32) -> Result<Vec<ServerMessage>> {
        let _ = self.sessions.remove(&session_id);
        Ok(vec![ServerMessage::Close { session_id }])
    }

    fn process_request(&mut self, request: &str) -> Result<Vec<ServerMessage>> {
        if let Some(caps) = self.regexes["connect"].captures(request).unwrap() {
            let session_id = caps["session_id"].parse()?;
            Ok(self.connect_session(session_id)?)
        } else if let Some(caps) = self.regexes["data"].captures(request)? {
            let session_id = caps["session_id"].parse()?;
            let pos = caps["pos"].parse()?;
            let data = &caps["data"];
            Ok(self.receive_data(session_id, pos, data)?)
        } else if let Some(caps) = self.regexes["ack"].captures(request)? {
            let session_id = caps["session_id"].parse()?;
            let pos = caps["pos"].parse()?;
            Ok(self.receive_ack(session_id, pos)?)
        } else if let Some(caps) = self.regexes["close"].captures(request)? {
            let session_id = caps["session_id"].parse()?;
            Ok(self.close_session(session_id)?)
        } else {
            Ok(Vec::new())
        }
    }
}

pub struct Server {
    state: Arc<Mutex<ServerState>>,
    ack_tasks: Arc<Mutex<HashMap<u32, JoinHandle<()>>>>,
}
impl Server {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ServerState::new())),
            ack_tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn acknowledge(&self, session_id: u32) {
        let mut addr_acks = self.ack_tasks.lock().await;
        if let Some(thread) = addr_acks.get_mut(&session_id) {
            thread.abort();
            let _ = addr_acks.remove(&session_id);
        }
    }

    async fn send_data(
        &self,
        socket: &Arc<UdpSocket>,
        addr: &SocketAddr,
        session_id: u32,
        data: String,
    ) {
        let socket = socket.clone();
        let addr = addr.clone();
        let ack_tasks_copy = Arc::clone(&self.ack_tasks);
        let state = Arc::clone(&self.state);
        let thread = tokio::spawn(async move {
            Self::send_message_loop(socket, addr, data.as_bytes().to_vec()).await;
            Self::close_session(session_id, ack_tasks_copy, state).await;
        });
        let mut ack_tasks = self.ack_tasks.lock().await;
        ack_tasks.insert(session_id, thread);
    }

    async fn send_message_loop(socket: Arc<UdpSocket>, addr: SocketAddr, data: Vec<u8>) {
        let mut interval = time::interval(time::Duration::from_millis(500));
        for _ in 0..=20 {
            interval.tick().await;
            println!(
                "{addr:?} --> {}",
                String::from_utf8_lossy(&data).replace("\n", r"\n")
            );
            let _ = socket.send_to(data.as_slice(), addr).await;
        }
    }

    async fn close_session(
        session_id: u32,
        addr_acks: Arc<Mutex<HashMap<u32, JoinHandle<()>>>>,
        state: Arc<Mutex<ServerState>>,
    ) {
        let mut addr_acks = addr_acks.lock().await;
        if let Some(thread) = addr_acks.get_mut(&session_id) {
            thread.abort();
            let _ = addr_acks.remove(&session_id);
        }

        let _ = state.lock().await.close_session(session_id);
    }
}

#[async_trait]
impl UdpServer for Server {
    async fn handle_connection(&self, socket: Arc<UdpSocket>, data: &[u8], addr: &SocketAddr) {
        let request = String::from_utf8_lossy(data);
        let request = request.trim();
        println!("{addr:?} <-- {}", request.replace("\n", r"\n"));

        let mut state = self.state.lock().await;
        let Ok(responses) = state.process_request(&request) else {
            return;
        };
        for response in responses {
            match response {
                ServerMessage::Ack { session_id } => self.acknowledge(session_id).await,

                ServerMessage::Data { session_id, data } => {
                    if data.starts_with("/ack/") {
                        println!("{addr:?} --> {}", data.replace("\n", r"\n"));
                        let _ = socket.send_to(data.as_bytes(), addr).await;
                    } else {
                        self.send_data(&socket, &addr, session_id, data).await
                    }
                }

                ServerMessage::Close { session_id } => {
                    let msg = format!("/close/{session_id}/");
                    let _ = socket.send_to(msg.as_bytes(), addr).await;
                }
            }
        }
    }
}
