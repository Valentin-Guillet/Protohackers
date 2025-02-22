use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::thread;
use std::{env, fs};

mod server_00;
mod server_01;

const SERVERS: [fn(TcpStream); 2] = [
    server_00::run as fn(TcpStream),
    server_01::run as fn(TcpStream),
];

fn get_part() -> Result<u8, &'static str> {
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

fn get_ip() -> Result<String, &'static str> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(r"ip -f inet addr show wlo1 | sed -En -e 's/.*inet ([0-9.]+).*/\1/p'")
        .output()
        .map_err(|_| "could not read ip")?;
    let ip = String::from_utf8(output.stdout).map_err(|_| "could not parse ip")?;
    Ok(ip.trim().to_string())
}

fn run_server(ip: &str, port: u32, server_index: usize) {
    println!("Running server {server_index:02}");
    let listener = TcpListener::bind(format!("{ip}:{port}")).unwrap();
    for stream in listener.incoming() {
        println!("Connection established!");
        thread::spawn(move || SERVERS[server_index](stream.unwrap()));
    }
}

pub fn get_server() -> Result<Box<dyn Fn()>, &'static str> {
    let part = get_part()? as usize;

    if part >= SERVERS.len() {
        return Err("part not found");
    }
    let ip = get_ip()?;
    let port = 12233;
    Ok(Box::new(move || run_server(&ip, port, part)))
}
