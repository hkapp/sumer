use std::{io::Write, os::unix::net::UnixListener, path::PathBuf, time::Duration, fs};

use common::{shm::SharedMemory, uds};

fn main() {
    let my_pos = std::env::args().next().unwrap();
    let mut dir = PathBuf::from(my_pos);
    dir.pop();
    dir.push("uds");
    // Ignore error if the file doesn't exist
    let _ = std::fs::remove_file(&dir);

    let listener = UnixListener::bind(&dir).unwrap();
    println!("Listening to connections on {}", dir.display());

    match listener.incoming().next() {
        Some(Ok(mut stream)) => {
            println!("Connection successful");
            stream.set_write_timeout(Some(Duration::from_secs(1))).unwrap();

            let file_to_read = uds::read_null_terminated_string(&mut stream).unwrap();
            println!("Reading {file_to_read} ...");

            // Read the data file
            let data_to_send = fs::read_to_string(file_to_read).unwrap();
            let data_size = data_to_send.len(); // Returns the number of bytes

            // Open the shared memory and write a basic message
            let target_name = "abc\0";
            let mut shm_mem = unsafe {
                SharedMemory::new(target_name, data_size)
                .unwrap()
            };

            let write_channel = unsafe { shm_mem.as_slice_mut() };
            write_channel.copy_from_slice(data_to_send.as_bytes());

            println!("Wrote {data_size} bytes to shared memory {target_name}");

            // Send the shared memory info to the client
            stream.write_all(&data_size.to_ne_bytes()).unwrap();
            uds::write_string_null_terminate(&mut stream, target_name).unwrap();
        }
        _ => {
            println!("Something went wrong");
        }
    }
}
