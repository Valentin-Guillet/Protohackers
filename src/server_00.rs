use std::{
    io::{Read, Write},
    net::TcpStream,
};

pub fn run(mut stream: TcpStream) {
    loop {
        let mut buffer = [0; 1024];
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => stream.write_all(&buffer[0..n]).unwrap(),
            Err(err) => panic!("{}", err),
        }
    }
}
