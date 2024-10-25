use std::{io::{Read, Write}, mem::size_of, os::unix::net::UnixListener, path::PathBuf, time::Duration};

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

            let my_message = b"This is a message from the server";
            stream.write_all(&my_message.len().to_ne_bytes()).unwrap();
            stream.write_all(my_message).unwrap();

            let mut len_buffer = [0; size_of::<usize>()];
            stream.read_exact(&mut len_buffer).unwrap();
            let client_len = usize::from_ne_bytes(len_buffer);

            let mut message_buffer = vec![0; client_len];
            stream.read_exact(&mut message_buffer).unwrap();
            let client_message = std::str::from_utf8(&message_buffer).unwrap();
            println!("{client_message}");
        }
        _ => {
            println!("Something went wrong");
        }
    }
}
