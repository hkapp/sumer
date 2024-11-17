// TODO make the following doc be for the module itself
/// An inter-process single reader single writer ring buffer over shared memory

/* Common */

use std::cmp::min;
use std::mem::size_of;
use std::slice;
use std::sync::atomic::{self, AtomicU64};
use std::time::Duration;

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
    PartnerDisconnected,
    /// Numeric value does not correspond to any known status
    InvalidStatus(u64),
    /// Must call `prepare_memory` before attempting to create streams
    MemoryNotPrepared,
}

impl ShmHeaderFormat {
    unsafe fn from_raw_mut<'a>(shm_ptr: *mut u8, shm_len: usize) -> Result<&'a mut Self, Error> {
        if shm_len > size_of::<Self>() {
            Ok(&mut *shm_ptr.cast())
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

#[derive(Debug, PartialEq, Eq)]
enum Status {
    NotConnected,
    Connected,
}

const NOT_CONNECTED_STATUS: u64 = 0;
const CONNECTED_STATUS:     u64 = 1;

impl TryFrom<u64> for Status {
    type Error = Error;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            NOT_CONNECTED_STATUS => Ok(Status::NotConnected),
            CONNECTED_STATUS     => Ok(Status::Connected),
            _                    => Err(Error::InvalidStatus(value)),
        }
    }
}

impl From<Status> for u64 {
    fn from(value: Status) -> Self {
        match value {
            Status::NotConnected => NOT_CONNECTED_STATUS,
            Status::Connected    => CONNECTED_STATUS,
        }
    }
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




/* Common header row API */

// Write-only, never need to read
struct MyRow {
    row_ptr: *mut ShmUsefulRow
}

impl MyRow {
    unsafe fn write_tot_count(&mut self, tot_count: u64) {
        ShmUsefulRow::atomic_count(self.row_ptr)
            .store(tot_count, ATOMIC_ORDER);
    }
}

impl From<*mut ShmUsefulRow> for MyRow {
    fn from(value: *mut ShmUsefulRow) -> Self {
        Self {
            row_ptr: value
        }
    }
}

// Read-only, should never write
struct PartnerRow {
    row_ptr: *mut ShmUsefulRow
}

impl PartnerRow {
    unsafe fn read_count(&self) -> Result<u64, Error> {
        let partner_pos = ShmUsefulRow::atomic_count(self.row_ptr)
                                .load(ATOMIC_ORDER);
        // Check the status after reading the position
        // If the partner aborts in between the two reads,
        // the pos value can be complete garbage
        self.check_status()
            .map(|_| partner_pos)
    }

    unsafe fn check_status(&self) -> Result<(), Error> {
        let status: Status = ShmUsefulRow::atomic_status(self.row_ptr)
                                .load(ATOMIC_ORDER)
                                .try_into()?;
        if status != Status::Connected {
            Err(Error::PartnerDisconnected)
        }
        else {
            Ok(())
        }
    }

    unsafe fn wait_for_count_change(&self, known_count: u64) -> Result<u64, Error> {
        // Wait for any change on their side.
        // We'll return the first value that change, whatever it is.
        let mut curr_count = self.read_count()?;
        let mut waiter = ExpWait::new();
            while curr_count == known_count {
            // TODO introduce a timeout
            waiter.wait();
            curr_count = self.read_count()?;
        }
        Ok(curr_count)
    }
}

impl From<*mut ShmUsefulRow> for PartnerRow {
    fn from(value: *mut ShmUsefulRow) -> Self {
        Self {
            row_ptr: value
        }
    }
}

/* Writer */
// TODO add a discussion about safety at the top of the module
pub struct StreamWriter {
    /// Where the data portion starts
    anchor_ptr:            *mut u8,
    /// Length of the data portion
    data_len:              usize,
    tot_bytes_written:     u64,
    cached_tot_bytes_read: u64,
    partner_row:           PartnerRow,
    my_row:                MyRow
}

// Note: implementing the io::Write trait would be deceiving,
// as all our APIs are unsafe
impl StreamWriter {
    pub unsafe fn write_all(&mut self, buf: &[u8]) -> Result<(), Error> {
        let mut still_to_write = buf;
        while !still_to_write.is_empty() {
            let write_target = self.contiguous_write_slice_blocking()?;
            let write_len = min(write_target.len(), still_to_write.len());
            let (write_into_now, _write_into_later) = write_target.split_at_mut(write_len);
            let (write_from_now, write_from_later) = still_to_write.split_at(write_len);
            write_into_now.copy_from_slice(write_from_now);
            self.wrote(write_len);
            still_to_write = write_from_later;
        }
        Ok(())
    }

    /// Returned slice is guaranteed to not be empty
    /// Can fail if the reader disconnected
    unsafe fn contiguous_write_slice_blocking(&mut self) -> Result<&mut [u8], Error> {
        if !self.free_write_space_cached() {
            self.wait_for_write_space()?;
        }
        Ok(self.contiguous_write_slice_non_blocking())
    }

    // Only reads info from cache
    unsafe fn contiguous_write_slice_non_blocking(&mut self) -> &mut [u8] {
        let slice_start = (self.tot_bytes_written % self.data_len as u64) as usize;
        let slice_end = (self.tot_bytes_written - self.cached_tot_bytes_read) as usize;
        let start_ptr = self.anchor_ptr.add(slice_start);
        slice::from_raw_parts_mut(start_ptr, slice_end - slice_start)
    }

    // May fail if the reader is no longer reading
    unsafe fn update_cache(&mut self) -> Result<(), Error> {
        self.cached_tot_bytes_read = self.partner_row.read_count()?;
        Ok(())
    }

    // Notifies that a certain number of bytes have been written
    unsafe fn wrote(&mut self, byte_count: usize) {
        // TODO add a compile time assert to validate that this conversion makes sense
        self.tot_bytes_written += byte_count as u64;
        self.my_row.write_tot_count(self.tot_bytes_written);
    }

    fn free_write_space_cached(&self) -> bool {
        (self.tot_bytes_written - self.cached_tot_bytes_read) < self.data_len as u64
    }

    /// Can fail if the reader disconnected
    unsafe fn wait_for_write_space(&mut self) -> Result<(), Error> {
        // TODO implement a timeout
        // TODO implement an adaptive wait based on reader/writer speed
        // TODO introduce a minimum write space
        self.cached_tot_bytes_read = self.partner_row.wait_for_count_change(self.cached_tot_bytes_read)?;
        Ok(())
    }
}

/* Reader */

/* Builder */

pub struct MemNotBigEnough(usize);

const HEADER_SIZE: usize = size_of::<ShmHeaderFormat>();

/// Warning: must only be done once
/// Must be done before calling either new_writer or new_reader
pub unsafe fn prepare_memory(addr: *mut u8, mem_sz: usize) -> Result<(), MemNotBigEnough> {
    if mem_sz >= HEADER_SIZE {
        // Rust's memset
        addr.write_bytes(0, HEADER_SIZE);
        Ok(())
    }
    else {
        Err(MemNotBigEnough(HEADER_SIZE))
    }
}

pub struct BuildWriter(StreamWriter);

impl BuildWriter {
    /// Memory must have been prepared with `prepare_memory`
    pub unsafe fn new(addr: *mut u8, mem_sz: usize) -> Result<Self, Error> {
        let header = ShmHeaderFormat::from_raw_mut(addr, mem_sz)?;
        let my_row = &mut header.writer_row.useful;

        // Validate that memory is cleared out
        if my_row.count.load(ATOMIC_ORDER) != 0
            || my_row.length.load(ATOMIC_ORDER) != 0
            || my_row.status.load(ATOMIC_ORDER) != 0
        {
            return Err(Error::MemoryNotPrepared);
        }

        // Write our header data in order
        // 1. length
        // TODO validate that the cast is ok
        my_row.length.store(mem_sz as u64, ATOMIC_ORDER);
        // 2. status
        // TODO introduce the following statuses: handhsake, aborted
        // then rename connected to working
        // TODO turn this into a function of the row
        my_row.status.store(Status::Connected.into(), ATOMIC_ORDER);

        let writer = StreamWriter {
            anchor_ptr: addr,
            data_len: mem_sz - HEADER_SIZE,
            tot_bytes_written: 0,
            cached_tot_bytes_read: 0,
            partner_row: PartnerRow::from(&mut header.reader_row.useful as *mut _),
            my_row: MyRow::from(my_row as *mut _),
        };
        Ok(BuildWriter(writer))
    }

    pub unsafe fn is_ready(&self) -> Result<bool, Error> {
        // TODO validate the length
        match self.0.partner_row.check_status() {
            Ok(()) => Ok(true), // partner is connected
            Err(Error::PartnerDisconnected) => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub unsafe fn blocking_into(self) -> Result<StreamWriter, Error> {
        self.wait_until_connected()?;
        Ok(self.0)
    }

    unsafe fn wait_until_connected(&self) -> Result<(), Error> {
        let mut waiter = ExpWait::new();
        while !self.is_ready()? {
            // TODO introduce a timeout
            waiter.wait();
        }
        Ok(())
    }
}
