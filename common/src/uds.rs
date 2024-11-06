use std::{io::{self, Read, Write}, string::FromUtf8Error, slice};

// FIXME this is extremely inefficient, we're reading bytes one by one
fn read_one<T: Read>(reader: &mut T) -> io::Result<u8> {
    let mut single_byte: u8 = 0;
    reader.read_exact(slice::from_mut(&mut single_byte))?;
    Ok(single_byte)
}

#[derive(Debug)]
pub enum Error {
    ReadError(io::Error),
    InputNotUtf8(FromUtf8Error),
}

pub fn read_null_terminated_string<T: Read>(reader: &mut T) -> Result<String, Error> {
    let mut buffer = Vec::new();
    loop {
        let c = read_one(reader)
                        .map_err(|io_err| Error::ReadError(io_err))?;
        if c == b'\0' {
            // End of the string
            return String::from_utf8(buffer)
                    .map_err(|conv_err| Error::InputNotUtf8(conv_err));
        }
        else {
            buffer.push(c);
        }
    }
}

pub fn write_string_null_terminate<W: Write>(writer: &mut W, message: &str) -> io::Result<()> {
    writer.write_all(message.as_bytes())?;
    writer.write_all(b"\0")
}
