use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::thread;

use crate::utils;

fn get_response(data: &mut Vec<(i32, i32)>, buf: &[u8]) -> Option<i32> {
    let query_type: char = char::from(buf[0]);
    let first = i32::from_be_bytes(buf[1..5].try_into().unwrap());
    let second = i32::from_be_bytes(buf[5..].try_into().unwrap());
    println!("Request: {} - {} - {}", query_type, first, second);

    match query_type {
        'Q' => {
            let (sum, count) = data
                .iter()
                .filter(|(timestamp, _)| (first..=second).contains(timestamp))
                .fold((0, 0), |(sum, count), &(_, price)| {
                    (sum + (price as i64), count + 1)
                });
            if count > 0 {
                Some((sum / count) as i32)
            } else {
                Some(0)
            }
        }
        'I' => {
            data.push((first, second));
            None
        }
        _ => None,
    }
}

fn handle_connection(mut stream: TcpStream) {
    let mut data = Vec::new();
    let mut buffer: Vec<u8> = Vec::new();
    while let Some(request) = utils::read_for(&mut stream, &mut buffer, 9) {
        println!("Request: {:?}", request);
        let response = get_response(&mut data, &request);
        if response.is_some() && stream.write_all(&response.unwrap().to_be_bytes()).is_err() {
            break;
        }
    }
}

pub fn run(ip: &str, port: u32) {
    println!("Running server 02");
    let listener = TcpListener::bind(format!("{ip}:{port}")).unwrap();
    for stream in listener.incoming() {
        println!("Connection established!");
        thread::spawn(move || handle_connection(stream.unwrap()));
    }
}
