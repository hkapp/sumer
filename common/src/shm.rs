use libc::{MAP_SHARED, O_RDWR, O_CREAT, PROT_WRITE, S_IRUSR, S_IWUSR};
use libc::{c_char, off_t, c_int};
use std::{ptr, slice};

type Fd = c_int;

pub struct SharedMemory {
    fd:        Fd,
    data_ptr:  *mut u8,
    data_size: usize
}

#[derive(Debug)]
pub enum Error {
    StringNotAscii,
    StringNotNullTerminated,
    StringEmpty
}

impl SharedMemory {
    pub unsafe fn new(shm_name: &str, shm_size: usize) -> Result<Self, Error> {
        // Validate the passed name
        if !shm_name.is_ascii() {
            return Err(Error::StringNotAscii)
        }
        let name_bytes = shm_name.as_bytes();

        match name_bytes.last() {
            None       => return Err(Error::StringEmpty),
            Some(b'\0') => {/* valid, nothing to do */},
            Some(_)    => return Err(Error::StringNotNullTerminated),
        }
        let c_name = name_bytes.as_ptr() as *const c_char;

        let null = ptr::null_mut();
        let fd   = libc::shm_open(c_name, O_RDWR | O_CREAT, S_IRUSR | S_IWUSR);
        let _res = libc::ftruncate(fd, shm_size as off_t);
        let addr = libc::mmap(null, shm_size, PROT_WRITE, MAP_SHARED, fd, 0);

        Ok(
            Self {
                fd,
                data_ptr:  addr as *mut u8,
                data_size: shm_size
            }
        )
    }

    pub unsafe fn as_slice(&self) -> &[u8] {
        slice::from_raw_parts(self.data_ptr, self.data_size)
    }

    pub unsafe fn as_slice_mut(&mut self) -> &mut [u8] {
        slice::from_raw_parts_mut(self.data_ptr, self.data_size)
    }
}

impl Drop for SharedMemory {
    fn drop(&mut self) {
        unsafe {
            // TODO call shm_unlink?
            libc::close(self.fd);
        }
    }
}
