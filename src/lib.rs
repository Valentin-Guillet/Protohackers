use std::net::SocketAddr;
use std::process::Command;
use std::sync::Arc;
use std::{env, fs};

use async_trait::async_trait;
use tokio::net::{TcpListener, TcpStream, UdpSocket};

mod server_00;
mod server_01;
mod server_02;
mod server_03;
mod server_04;
mod server_05;
mod server_06;
mod server_07;
mod server_08;
mod server_09;
mod server_10;
mod utils;

pub fn get_challenge() -> Result<u8, &'static str> {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 2 {
        return args[1].parse().or(Err("parsing error"));
    }

    fs::read_dir("./src/")
        .unwrap()
        .filter_map(|entry| {
            entry.ok().and_then(|e| {
                e.file_type()
                    .is_ok_and(|e| e.is_file())
                    .then(|| {
                        let file_name = e.file_name();
                        let file_name = file_name.to_str()?;
                        file_name
                            .strip_prefix("server_")?
                            .strip_suffix(".rs")?
                            .parse::<u8>()
                            .ok()
                    })
                    .flatten()
            })
        })
        .max()
        .ok_or("no source file found")
}

pub fn get_ip() -> Result<String, &'static str> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(r"ip -f inet addr show wlo1 | sed -En -e 's/.*inet ([0-9.]+).*/\1/p'")
        .output()
        .map_err(|_| "could not read ip")?;
    let ip = String::from_utf8(output.stdout).map_err(|_| "could not parse ip")?;
    Ok(ip.trim().to_string())
}

#[async_trait]
pub trait TcpServer: Send + Sync {
    async fn handle_connection(&self, mut stream: TcpStream);
}

#[async_trait]
pub trait UdpServer: Send + Sync {
    async fn handle_connection(&self, socket: Arc<UdpSocket>, data: &[u8], addr: &SocketAddr);
}

pub enum ServerType {
    Tcp(Arc<dyn TcpServer>),
    Udp(Arc<dyn UdpServer>),
}

pub struct Server {
    part: u8,
    server: ServerType,
}

impl Server {
    pub fn new(part: u8) -> Result<Self, &'static str> {
        let server = match part {
            0 => ServerType::Tcp(Arc::new(server_00::Server::new())),
            1 => ServerType::Tcp(Arc::new(server_01::Server::new())),
            2 => ServerType::Tcp(Arc::new(server_02::Server::new())),
            3 => ServerType::Tcp(Arc::new(server_03::Server::new())),
            4 => ServerType::Udp(Arc::new(server_04::Server::new())),
            5 => ServerType::Tcp(Arc::new(server_05::Server::new())),
            6 => ServerType::Tcp(Arc::new(server_06::Server::new())),
            7 => ServerType::Udp(Arc::new(server_07::Server::new())),
            8 => ServerType::Tcp(Arc::new(server_08::Server::new())),
            9 => ServerType::Tcp(Arc::new(server_09::Server::new())),
            10 => ServerType::Tcp(Arc::new(server_10::Server::new())),
            _ => return Err("invalid challenge number"),
        };
        Ok(Self { part, server })
    }

    pub async fn run(self, ip: &str, port: u32) {
        println!("Running server {}", self.part);
        match self.server {
            ServerType::Tcp(server) => Self::run_tcp(server, ip, port).await,
            ServerType::Udp(server) => Self::run_udp(server, ip, port).await,
        }
    }

    async fn run_tcp(server: Arc<dyn TcpServer>, ip: &str, port: u32) {
        let listener = TcpListener::bind(format!("{ip}:{port}")).await.unwrap();

        loop {
            let (stream, _) = listener.accept().await.unwrap();
            println!("Connection established!");

            let server = Arc::clone(&server);
            tokio::spawn(async move { server.handle_connection(stream).await });
        }
    }

    async fn run_udp(server: Arc<dyn UdpServer>, ip: &str, port: u32) {
        let socket = Arc::new(UdpSocket::bind(format!("{ip}:{port}")).await.unwrap());
        loop {
            let mut buffer = [0; 1024];
            let (n, addr) = socket.recv_from(&mut buffer).await.unwrap();
            let server = Arc::clone(&server);
            let socket = Arc::clone(&socket);
            tokio::spawn(
                async move { server.handle_connection(socket, &buffer[..n], &addr).await },
            );
        }
    }
}
