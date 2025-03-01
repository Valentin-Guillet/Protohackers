use std::process::Command;
use std::{env, fs};

mod utils;

macro_rules! load_server {
    (@replace_expr $s:ident $sub:expr) => { $sub };
    (@count_servers $($server:ident)*) => { 0usize $(+ load_server!(@replace_expr $server 1usize))* };

    ($($server:ident),*) => {
        $(mod $server;)*
        const NB_SERVERS: usize = load_server!(@count_servers $($server)*);
        const SERVER_RUNS: [fn(&str, u32); NB_SERVERS] = [
            $($server::run as fn(&str, u32),)*
        ];
    };
}

load_server!(server_00, server_01, server_02, server_03);

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

pub fn get_server() -> Result<Box<dyn Fn()>, &'static str> {
    let part = get_part()? as usize;

    if part >= SERVER_RUNS.len() {
        return Err("part not found");
    }
    let ip = get_ip()?;
    let port = 12233;
    Ok(Box::new(move || SERVER_RUNS[part](&ip, port)))
}
