use std::io::Read;
use std::net::TcpStream;

pub fn read_until(stream: &mut TcpStream, buffer: &mut [u8], limit: char) -> Option<String> {
    let limit = limit as u8;

    let mut data = String::new();
    let mut data_len = buffer
        .iter()
        .position(|&c| c == b'\0')
        .unwrap_or(buffer.len());

    while !buffer[..data_len].contains(&limit) {
        data.push_str(&String::from_utf8_lossy(&buffer[..data_len]));
        data_len = match stream.read(buffer) {
            Err(err) => panic!("{}", err),
            Ok(0) => return None,
            Ok(n) => n,
        };
    }

    let index = buffer.iter().position(|&c| c == limit).unwrap();
    data += &String::from_utf8_lossy(&buffer[..index]);
    buffer.copy_within(index + 1..data_len, 0);
    let remaining_len = data_len - index - 1;
    buffer[remaining_len..].fill(0);

    Some(data)
}

pub fn read_for(stream: &mut TcpStream, buffer: &mut Vec<u8>, nb_bytes: usize) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    let mut data_len = buffer.len();

    while data.len() + data_len < nb_bytes {
        data.extend(buffer.iter());
        let mut buf = [0; 1024];
        data_len = match stream.read(&mut buf) {
            Err(err) => panic!("{}", err),
            Ok(0) => return None,
            Ok(n) => n,
        };
        *buffer = buf[..data_len].to_vec();
    }

    data.extend(buffer.drain(..nb_bytes - data.len()));
    Some(data)
}
