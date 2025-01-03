use std::{mem::size_of, io::{self, Read}, os::unix::net::UnixStream, path::PathBuf, time::Duration, str};

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

            let file_to_read = "data/wiki.txt";
            println!("Asking to read {file_to_read}");
            uds::write_string_null_terminate(&mut stream, file_to_read).unwrap();

            // Receive shared memory info from the server
            let mut usize_buffer = [0; size_of::<usize>()];
            stream.read_exact(&mut usize_buffer).unwrap();
            let shm_size = usize::from_ne_bytes(usize_buffer);

            let mut shm_name = uds::read_null_terminated_string(&mut stream).unwrap();
            // The string is read without a null terminator
            // Add the null terminator now to make C APIs happy
            shm_name.push('\0');
            println!("Reading {shm_size} bytes from shared memory {shm_name}");

            // Link to shared memory and read data from the server
            let shm_mem = unsafe {
                SharedMemory::new(&shm_name, shm_size)
                    .unwrap()
            };

            let read_channel = unsafe { shm_mem.as_slice() };
            let shm_message = str::from_utf8(read_channel).unwrap();
            assert!(shm_message.is_ascii());
            println!("{shm_message}");
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
