// TODO make the following doc be for the module itself
/// An inter-process single reader single writer ring buffer over shared memory

/* Common */

use std::{mem::size_of, sync::atomic::{self, AtomicU64}, time::Duration};

/// A single row in the header of the shared memory area
#[repr(C)]
struct ShmUsefulRow {
    status: AtomicU64,  // Note: AtomicU64::from_ptr() is currently unstable
    length: AtomicU64,
    count:  AtomicU64,
}

impl ShmUsefulRow {
    unsafe fn atomic_status<'a>(me: *mut Self) -> &'a AtomicU64 {
        &(*me).status
    }

    unsafe fn atomic_length<'a>(me: *mut Self) -> &'a AtomicU64 {
        &(*me).length
    }

    unsafe fn atomic_count<'a>(me: *mut Self) -> &'a AtomicU64 {
        &(*me).count
    }
}

// For the cache line size, see https://stackoverflow.com/questions/794632/programmatically-get-the-cache-line-size
// This is the value on my current machine
const CACHE_LINE_SIZE: usize = 64;
const PADDING_AMOUNT: usize = CACHE_LINE_SIZE - size_of::<ShmUsefulRow>();

#[repr(C)]
struct ShmFullRow {
    useful: ShmUsefulRow,
    padding: [u8; PADDING_AMOUNT],
}

// TODO add a compile time assertion that sizeof(ShmFullRow) == CACHE_LINE_SIZE

#[repr(C)]
struct ShmHeaderFormat {
    reader_row: ShmFullRow,
    writer_row: ShmFullRow,
}

// TODO move around
pub enum Error {
    SharedMemoryNotLargeEnough,
    HandshakeFailed,
}

impl ShmHeaderFormat {
    fn validated_cast(shm_ptr: *mut u8, shm_len: usize) -> Result<*mut Self, Error> {
        if shm_len > size_of::<Self>() {
            Ok(shm_ptr.cast())
        }
        else {
            Err(Error::SharedMemoryNotLargeEnough)
        }
    }

    unsafe fn reader_ptr(me: *mut Self) -> *mut ShmUsefulRow {
        &mut (*me).reader_row.useful as *mut _
    }

    unsafe fn writer_ptr(me: *mut Self) -> *mut ShmUsefulRow {
        &mut (*me).writer_row.useful as *mut _
    }

    unsafe fn handshake_channel<'a>(me: *mut Self) -> &'a AtomicU64 {
        let writer_row = Self::writer_ptr(me);
        ShmUsefulRow::atomic_status(writer_row)
    }
}

/* Handshake protocol */

enum Status {
    WriterReady,
    ReaderAlsoReady,
    Writing,
    Reading,
    Abort
}

const WRITER_READY_STATUS: u64 = 0;
const READER_ALSO_READY_STATUS: u64 = 1;
const WRITING_STATUS: u64 = 2;
const READING_STATUS: u64 = 3;
const ABORT_STATUS: u64 = 4;

// TODO this function requires a timeout
unsafe fn writer_handshake(header: *mut ShmHeaderFormat, length: usize) -> Result<(), Error> {
    // Step 1: set up our side
    init_row(ShmHeaderFormat::writer_ptr(header), length);

    // Step 2: write "READY"
    let handshake_channel = ShmHeaderFormat::handshake_channel(header);
    handshake_channel.store(WRITER_READY_STATUS, ATOMIC_ORDER);

    let abort = || {
        handshake_channel.store(ABORT_STATUS, ATOMIC_ORDER);
        Err(Error::HandshakeFailed)
    };

    // Step 3: wait for "READER_ALSO_READY"
    let handhsake_value = wait_for_change(handshake_channel, READER_ALSO_READY_STATUS);
    if handhsake_value != READER_ALSO_READY_STATUS {
        // Abort the handshake
        return abort();
    }

    // Step 4: validate the reader's length field
    let reader_row = ShmHeaderFormat::reader_ptr(header);
    let reader_length = ShmUsefulRow::atomic_length(reader_row).load(ATOMIC_ORDER);
    if reader_length != length as u64 {
        // We don't agree with the reader on the length of the shared memory
        // Abort the handshake
        return abort();
    }

    // Step 5: write "WRITING"
    match handshake_channel.compare_exchange(READER_ALSO_READY_STATUS, WRITING_STATUS, ATOMIC_ORDER, ATOMIC_ORDER) {
        Ok(_) => Ok(()), // handshake successful
        Err(_) => {
            // Someone wrote something in the middle of our handshake
            // Fail the handshake
            abort()
        }
    }
}

const MAGIC_NUMBER: u64 = 0xbabe101ebabe101e;
const ATOMIC_ORDER: atomic::Ordering = atomic::Ordering::Relaxed;

/// Set up the non-status fields of the given row header
/// This must be performed before starting the handshake
unsafe fn init_row(row: *mut ShmUsefulRow, length: usize) {
    // TODO make this into a compile-time constant
    assert!(size_of::<usize>() <= size_of::<u64>());
    ShmUsefulRow::atomic_length(row).store(length as u64, ATOMIC_ORDER);
    ShmUsefulRow::atomic_count(row).store(0, ATOMIC_ORDER);
}

struct ExpWait {
    curr_wait: Duration
}

impl ExpWait {
    fn new() -> Self {
        Self {
            curr_wait: Duration::from_nanos(10)
        }
    }

    fn wait(&mut self) {
        // TODO check the implementation of Rust channels to know how they wake up
        std::thread::sleep(self.curr_wait);
        self.curr_wait = self.curr_wait.saturating_mul(2);
    }
}

/// Wait for any concurrent change at the given memory location.
/// Returns early if some expected value is found.
fn wait_for_change(channel: &AtomicU64, early_stop: u64) -> u64 {
    let initial_value = channel.load(ATOMIC_ORDER);

    if initial_value == early_stop {
        return initial_value;
    }

    // Wait for any change on their side.
    // We'll return the first value that change, whatever it is.
    let mut latest_value = channel.load(ATOMIC_ORDER);
    let mut waiter = ExpWait::new();
    while latest_value == initial_value {
        // TODO introduce a timeout
        waiter.wait();
        latest_value = channel.load(ATOMIC_ORDER);
    }

    return latest_value;
}

/* Writer */

pub struct Writer {

}

impl Writer {
    pub unsafe fn new_blocking(shm_ptr: *mut u8, shm_len: usize) -> Result<Self, Error> {
        let shm_fmt = ShmHeaderFormat::validated_cast(shm_ptr, shm_len)?;
        writer_handshake(shm_fmt, shm_len)?;
        Ok(Writer{})
    }
}

/* Reader */
