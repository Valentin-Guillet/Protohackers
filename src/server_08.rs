use std::ops::BitXor;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::TcpServer;

fn get_most_freq_toy(request: &str) -> &str {
    request
        .split(',')
        .max_by_key(|&toy| {
            toy.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u32>()
                .unwrap()
        })
        .unwrap()
}

struct Workshop {
    buffer: String,
}

impl Workshop {
    fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    fn add_data(&mut self, data: String) -> Vec<String> {
        self.buffer.push_str(&data);
        let mut toys = Vec::new();
        while let Some(index) = self.buffer.find('\n') {
            let toy_list = self.buffer.drain(..=index);
            let toy_list = &toy_list.as_str()[..index]; // remove last '\n' char
            toys.push(get_most_freq_toy(toy_list).to_string() + "\n");
        }
        toys
    }
}

enum CipherOp {
    Reversebits,
    Xor(u8),
    Xorpos,
    Add(u8),
    Addpos,
}

impl CipherOp {
    fn encode(&self, byte: u8, pos: usize) -> u8 {
        match self {
            CipherOp::Reversebits => byte.reverse_bits(),
            CipherOp::Xor(n) => byte.bitxor(n),
            CipherOp::Xorpos => byte.bitxor(pos as u8),
            CipherOp::Add(n) => byte.wrapping_add(*n),
            CipherOp::Addpos => byte.wrapping_add(pos as u8),
        }
    }

    // Decoding is the same as encoding with substraction instead of addition
    fn decode(&self, byte: u8, pos: usize) -> u8 {
        match self {
            CipherOp::Add(n) => byte.wrapping_sub(*n),
            CipherOp::Addpos => byte.wrapping_sub(pos as u8),
            _ => self.encode(byte, pos),
        }
    }

    fn parse_spec(spec: &[u8]) -> Vec<Self> {
        let mut cipher_ops = Vec::new();
        let mut index = 0usize;
        while index < spec.len() {
            cipher_ops.push(match spec[index] {
                0x00 => break,
                0x01 => CipherOp::Reversebits,
                0x02 => {
                    index += 1;
                    CipherOp::Xor(spec[index])
                }
                0x03 => CipherOp::Xorpos,
                0x04 => {
                    index += 1;
                    CipherOp::Add(spec[index])
                }
                0x05 => CipherOp::Addpos,
                _ => unreachable!(),
            });
            index += 1;
        }
        cipher_ops
    }
}

struct ObfuscationLayer {
    cipher_ops: Vec<CipherOp>,
    client_pos: usize,
    server_pos: usize,
}

impl ObfuscationLayer {
    fn new(spec: Vec<u8>) -> Result<Self> {
        let cipher_ops = CipherOp::parse_spec(&spec);

        let mut layer = Self {
            cipher_ops,
            client_pos: 0,
            server_pos: 0,
        };

        let equal_test = "123abcdefg789";
        if layer.encode(equal_test) != equal_test.as_bytes() {
            layer.server_pos = 0; // reset pos
            Ok(layer)
        } else {
            Err(anyhow!("ERROR: spec is a no-op cipher"))
        }
    }

    fn encode(&mut self, msg: &str) -> Vec<u8> {
        msg.bytes()
            .map(|c| {
                self.server_pos += 1;
                self.cipher_ops
                    .iter()
                    .fold(c, |byte, op| op.encode(byte, self.server_pos - 1))
            })
            .collect()
    }

    fn decode(&mut self, msg: &[u8]) -> String {
        String::from_utf8_lossy(
            &msg.iter()
                .map(|&b| {
                    self.client_pos += 1;
                    self.cipher_ops
                        .iter()
                        .rev()
                        .fold(b, |byte, op| op.decode(byte, self.client_pos - 1))
                })
                .collect::<Vec<_>>(),
        )
        .to_string()
    }
}

pub struct Server {}

impl Server {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl TcpServer for Server {
    async fn handle_connection(&self, mut stream: TcpStream) {
        let mut buffer = Vec::new();

        while !buffer.contains(&0) {
            let mut buf = [0; 1024];
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => return,
                Ok(n) => buffer.extend_from_slice(&buf[..n]),
            };
        }
        let index = buffer.iter().position(|&b| b == 0).unwrap();
        let cipher_spec = buffer.drain(..=index).collect();
        println!("Cipher spec: {:?}", cipher_spec);

        let Ok(mut obfuscation_layer) = ObfuscationLayer::new(cipher_spec) else {
            return;
        };

        let mut workshop = Workshop::new();
        loop {
            if buffer.is_empty() {
                let mut buf = [0; 1024];
                match stream.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buffer.extend_from_slice(&buf[..n]),
                };
            }

            let msg = obfuscation_layer.decode(&buffer);
            buffer.clear();
            println!("Decoded: {msg}");
            for resp_msg in workshop.add_data(msg) {
                println!("Response: {resp_msg}");
                let response = obfuscation_layer.encode(&resp_msg);
                stream.write_all(&response).await.unwrap();
            }
        }
    }
}
