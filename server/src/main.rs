use std::{os::unix::net::UnixListener, path::PathBuf, time::Duration};

use common::{shm::SharedMemory, uds};

fn main() {
    let my_pos = std::env::args().next().unwrap();
    let mut dir = PathBuf::from(my_pos);
    dir.pop();
    dir.push("uds");
    // Ignore error if the file doesn't exist
    let _ = std::fs::remove_file(&dir);

    let listener = UnixListener::bind(&dir).unwrap();
    println!("Listening to connections to {}", dir.display());

    match listener.incoming().next() {
        Some(Ok(mut stream)) => {
            println!("Connection successful");
            stream.set_write_timeout(Some(Duration::from_secs(1))).unwrap();

            let my_message = "This is a message from the server";
            uds::write_string_null_terminate(&mut stream, my_message).unwrap();

            let client_message = uds::read_null_terminated_string(&mut stream).unwrap();
            println!("{client_message}");

            // Open the shared memory and write a basic message
            // TODO send the name and size to the client
            let target_name = "abc\0";
            let data_size = 128;
            let mut shm_mem = unsafe {
                SharedMemory::new(target_name, data_size)
                    .unwrap()
            };
            let write_channel = unsafe { shm_mem.as_slice_mut() };

            for (i, x) in write_channel.iter_mut().enumerate() {
                *x = i as u8;
            }
        }
        _ => {
            println!("Something went wrong");
        }
    }
}
