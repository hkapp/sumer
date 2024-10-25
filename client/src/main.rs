use core::str;
use std::{io::{self, Read, Write}, mem::size_of, os::unix::net::UnixStream, path::PathBuf, time::Duration};

fn main() {
    let my_pos = std::env::args().next().unwrap();
    let mut dir = PathBuf::from(my_pos);
    dir.pop();
    dir.push("uds");

    println!("Connecting to {}", dir.display());

    match UnixStream::connect(&dir) {
        Ok(mut stream) => {
            println!("Connection successful");
            stream.set_write_timeout(Some(Duration::from_secs(1))).unwrap();

            let my_message = b"This is a message from the client";
            stream.write_all(&my_message.len().to_ne_bytes()).unwrap();
            stream.write_all(my_message).unwrap();

            let mut len_buffer = [0; size_of::<usize>()];
            stream.read_exact(&mut len_buffer).unwrap();
            let server_len = usize::from_ne_bytes(len_buffer);

            let mut message_buffer = vec![0; server_len];
            stream.read_exact(&mut message_buffer).unwrap();
            let server_message = str::from_utf8(&message_buffer).unwrap();
            println!("{server_message}");
        }
        Err(e) => {
            match e.kind() {
                io::ErrorKind::ConnectionRefused => {
                    println!("{}", e);
                    println!("This could be due to the server not running");
                }
                _ => {
                    println!("Something went wrong:");
                    // println!("{:?}", e);
                    println!("{}", e);
                }
            }
        }
    }
}
