// TODO make the following doc be for the module itself
/// An inter-process single reader single writer ring buffer over shared memory

/* Common */

use std::{mem::size_of, sync::atomic::{self, AtomicU64}};

/// A single row in the header of the shared memory area
#[repr(C)]
struct ShmUsefulRow {
    status: u64,
    length: u64,
    count:  u64,
}

impl ShmUsefulRow {
    unsafe fn atomic_status<'a>(me: *mut Self) -> &'a AtomicU64 {
        AtomicU64::from_ptr((*me).status)
    }

    unsafe fn atomic_length<'a>(me: *mut Self) -> &'a AtomicU64 {
        AtomicU64::from_ptr((*me).length)
    }

    unsafe fn atomic_count<'a>(me: *mut Self) -> &'a AtomicU64 {
        AtomicU64::from_ptr((*me).count)
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
enum Error {
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
}

/* Handshake protocol */

// TODO this function requires a timeout
unsafe fn writer_handshake(header: *mut ShmHeaderFormat, length: usize) {
    // Step 1: try to clear out the reader's data
    clear_opposite_data(ShmHeaderFormat::reader_ptr(header), length);
    // Step 2: set up the writer's side handshake
    setup_this_side(ShmHeaderFormat::writer_ptr(header), length);
    // Step 3: wait for the handshake to complete
    wait_for_handshake(ShmHeaderFormat::reader_ptr(header), length);
}

const MAGIC_NUMBER: u64 = 0xbabe101ebabe101e;
const ATOMIC_ORDER: atomic::Ordering = atomic::Ordering::Relaxed;

unsafe fn clear_opposite_data(row: *mut ShmUsefulRow, length: usize) -> Result<(), Error> {
    handshake_cas_twice(ShmUsefulRow::atomic_status(row), 0, MAGIC_NUMBER)?;
    // TODO turn this into a compile-time assert
    assert!(size_of<usize>() <= size_of<u64>());
    // Q: What happens if the other side got the wrong length here? We are messing them up.
    handshake_cas_twice(ShmUsefulRow::atomic_length(row), 0, length as u64)?;
    // Note: we don't need to clear the count, this is not part of the handshake
    Ok(())
}

unsafe fn setup_this_side(row: *mut ShmUsefulRow, length: usize) {
    // The order in this function is extremely important
    // It guarantees that the handshake cannot be misinterpreted by the other process

    // Start by the count: this is not part of the handshake
    ShmUsefulRow::atomic_count(row).store(0, ATOMIC_ORDER);

    // Now the length and status IN THIS ORDER
    handshake_cas_thrice(ShmUsefulRow::atomic_length(me), length as u64, 0);
    handshake_cas_thrice(ShmUsefulRow::atomic_status(me), MAGIC_NUMBER, 0);
}

fn handshake_cas_twice(target: &AtomicU64, want_to_store: u64, also_accept: u64) -> Result<u64, Error> {
    let previous_value = target.load(ATOMIC_ORDER);

    if previous_value == also_accept || previous_value == want_to_store {
        // Return early, we're good here
        return Ok(previous_value);
    }

    let cas =
        |prev_val| target.compare_exchange(prev_val, want_to_store, ATOMIC_ORDER, ATOMIC_ORDER);

    match cas(previous_value) {
        Ok(written) => Ok(written),
        Err(intermediate_value) => {
            if intermediate_value == also_accept {
                Ok(intermediate_value)
            }
            else if intermediate_value == want_to_store {
                // From the initial check in this function, we know
                // that the initial value was not `want_to_store`.
                // This means that someone else is writing to this shared
                // memory, and they're behaving like us (resp. reader or writer),
                // when they should behave the opposite (resp. writer or reader).
                // This fails the handshake.
                Err(Error::HandshakeFailed)
            }
            else {
                // Try the CAS again
                match cas(intermediate_value) {
                    Ok(written) => Ok(written),
                    Err(last_chance) => {
                        if last_chance == also_accept {
                            Ok(last_chance)
                        }
                        else {
                            // Someone wrote another value in the meantime,
                            // and the value is not `also_accept`.
                            // This means that someone else is writing into this shared
                            // memory. This fails the handshake.
                            // Note that this case also covers the scenario when the
                            // intermediate value is `want_to_store` (see comment above).
                            Err(Error::HandshakeFailed)
                        }
                    }
                }
            }
        }
    }
}

unsafe fn wait_for_handshake(partner_row: *mut ShmUsefulRow, length: usize) -> Result<(), Error> {
    let handshake_value = handshake_wait(ShmUsefulRow::atomic_status(partner_row))?;

    if handshake_value == MAGIC_NUMBER {
        // TODO: do we need a negative status magic number?
        // Our partner notified us.
        // Check that the length they know about is the same as ours
        // The order of writes in setup_this_side() guarantees that
        // this length value has been written
        let partner_length = ShmUsefulRow::atomic_length(partner_row).load(ATOMIC_ORDER);
        if partner_length == length as u64 {
            // Handshake successful
            Ok(())
        }
        else {
            // They have signalled us but we don't know about the same length.
            // This fails the handshake.
            // TODO we need to reset our status field upon handshake failure.
            Err(Error::HandshakeFailed)
        }
    }
    else {
        // Some other value was written in the handshake slot
        // Either someone else is writing to that memory,
        // or the opposite process doesn't know the protocol
        // This fails the handshake
        Err(Error::HandshakeFailed)
    }
}

/// The "wait" portion of the [wait_for_handshake] function
/// This function waits for activity or handshake success on the partner side.
fn handshake_wait(partner_status: &AtomicU64) -> u64 {
    let initial_value = partner_status.load(ATOMIC_ORDER);

    if initial_value == MAGIC_NUMBER {
        // They already signalled us.
        // Return early.
        return initial_value;
    }

    // Wait for any change on their side.
    // We'll return the first value that change, whatever it is.
    let mut latest_value = partner_status.load(ATOMIC_ORDER);
    while latest_value == initial_value {
        // TODO introduce a timeout
        waiter.wait();
        latest_value = partner_status.load(ATOMIC_ORDER);
    }

    return latest_value;
}

/// Write the given value at the target,
/// allowing exactly one seen value in the process
fn handshake_cas_thrice(target: &AtomicU64, want_to_store: u64, accepted_intermediate: u64) -> Result<(), Error> {
    // What happens if we had the magic number in our header initially?
    // A sensible reader would have cleared it
    // So this can be accepted
    // -> NO! They would not clear it in their evolved CAS
    // => the easiest to do the handshake correctly is to have each side write messages in the same shared variable
}

/* Writer */

pub struct Writer {

}

impl Writer {
    pub unsafe fn new_blocking(shm_ptr: *mut u8, shm_len: usize) -> Result<Self, Error> {
        let shm_fmt = ShmHeaderFormat::validated_cast(shm_ptr, shm_len)?;
        writer_handshake(shm_fmt, shm_len);
    }
}

/* Reader */
