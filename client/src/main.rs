use std::{io, os::unix::net::UnixStream, path::PathBuf, time::Duration};

use common::{shm::SharedMemory, uds};

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

            let my_message = "This is a message from the client";
            uds::write_string_null_terminate(&mut stream, my_message).unwrap();

            let server_message = uds::read_null_terminated_string(&mut stream).unwrap();
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
