use std::{
    io::{Read, Write},
    net::TcpStream,
};

pub fn run(mut stream: TcpStream) {
    loop {
        let mut read = [0; 1024];
        match stream.read(&mut read) {
            Ok(0) => break,
            Ok(n) => stream.write_all(&read[0..n]).unwrap(),
            Err(err) => panic!("{}", err),
        }
    }
}
