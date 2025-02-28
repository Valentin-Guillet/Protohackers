use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

fn handle_connection(mut stream: TcpStream) {
    loop {
        let mut buffer = [0; 1024];
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => stream.write_all(&buffer[0..n]).unwrap(),
            Err(err) => panic!("{}", err),
        }
    }
}

pub fn run(ip: &str, port: u32) {
    println!("Running server 00");
    let listener = TcpListener::bind(format!("{ip}:{port}")).unwrap();
    for stream in listener.incoming() {
        println!("Connection established!");
        thread::spawn(move || handle_connection(stream.unwrap()));
    }
}
