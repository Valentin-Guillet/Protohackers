use std::{env, fs};

mod server_00;

const SERVERS: [fn(); 1] = [server_00::run as fn()];

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

pub fn get_server() -> Result<fn(), &'static str> {
    const N: usize = SERVERS.len();
    match get_part()? as usize {
        part @ 0..N => Ok(*SERVERS.get(part).unwrap()),
        _ => Err("part not found"),
    }
}
