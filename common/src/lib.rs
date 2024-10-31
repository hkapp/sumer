pub mod datastream;
pub mod promise;

use libc::{close, ftruncate, mmap, shm_open};
use libc::{MAP_SHARED, O_RDWR, O_CREAT, PROT_WRITE, S_IRUSR, S_IWUSR};
use libc::{c_char, c_void, off_t, c_int};
use std::ptr;

type Fd = c_int;

pub unsafe fn share_memory(name: *const c_char, sz: usize) -> (Fd, *mut c_void) {
    let null = ptr::null_mut();
    let fd   = shm_open(name, O_RDWR | O_CREAT, (S_IRUSR | S_IWUSR));
    let _res = ftruncate(fd, sz as off_t);
    let addr = mmap(null, sz, PROT_WRITE, MAP_SHARED, fd, 0);

    (fd, addr)
}

pub unsafe fn unshare_memory(fd: Fd) {
    // TODO call shm_unlink?
    close(fd);
}
