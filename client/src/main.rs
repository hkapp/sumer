use core::str;
use std::{io::{self, Read, Write}, mem::size_of, os::unix::net::UnixStream, path::PathBuf, time::Duration};

use common::shm::SharedMemory;

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

            // Link to shared memory and read data from the server
            // FIXME this is currently a data race!
            let shm_name = "abc\0";
            let data_size = 128;
            let shm_mem = unsafe {
                SharedMemory::new(shm_name, data_size)
                    .unwrap()
            };
            let read_channel = unsafe { shm_mem.as_slice() };

            read_channel.iter()
                .for_each(|x| println!("{x}"));
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
