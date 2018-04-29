// Original implementation Copyright 2013 The Rust Project Developers <https://github.com/rust-lang>
//
// Original source file: https://github.com/rust-lang/rust/blob/master/src/libstd/io/buffered.P
//
// Additions copyright 2016-2018 Austin Bonander <austin.bonander@gmail.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
//! Drop-in replacements for buffered I/O types in `std::io`.
//!
//! These replacements retain the method names/signatures and implemented traits of their stdlib
//! counterparts, making replacement as simple as swapping the import of the type:
//!
//! #### `BufReader`:
//! ```notest
//! - use std::io::BufReader;
//! + use buf_redux::BufReader;
//! ```
//! #### `BufWriter`:
//! ```notest
//! - use std::io::BufWriter;
//! + use buf_redux::BufWriter;
//! ```
//! #### `LineWriter`:
//! ```notest
//! - use std::io::LineWriter;
//! + use buf_redux::LineWriter;
//! ```
//!
//! ### More Direct Control
//! All replacement types provide methods to:
//!
//! * Increase the capacity of the buffer
//! * Get the number of available bytes as well as the total capacity of the buffer
//! * Consume the wrapper without losing data
//!
//! `BufReader` provides methods to:
//!
//! * Access the buffer through an `&`-reference without performing I/O
//! * Force unconditional reads into the buffer
//! * Get a `Read` adapter which empties the buffer and then pulls from the inner reader directly
//! * Shuffle bytes down to the beginning of the buffer to make room for more reading
//! * Get inner reader and trimmed buffer with the remaining data
//!
//! `BufWriter` and `LineWriter` provides methods to:
//!
//! * Flush the buffer and unwrap the inner writer unconditionally.
//! * Get the inner writer and trimmed buffer with the unflushed data.
//!
//! ### More Sensible and Customizable Buffering Behavior
//! * Tune the behavior of the buffer to your specific use-case using the types in the [`strategy`
//! module](strategy/index.html):
//!     * `BufReader` performs reads as dictated by the [`ReadStrategy` trait](strategy/trait.ReadStrategy.html).
//!     * `BufReader` moves bytes down to the beginning of the buffer, to make more room at the end, when deemed appropriate by the
//! [`MoveStrategy` trait](strategy/trait.MoveStrategy.html).
//!     * `BufWriter` flushes bytes to the inner writer when full, or when deemed appropriate by
//!         the [`FlushStrategy` trait](strategy/trait.FlushStrategy.html).
//! * `Buffer` uses exact allocation instead of leaving it up to `Vec`, which allocates sizes in powers of two.
//!     * Vec's behavior is more efficient for frequent growth, but much too greedy for infrequent growth and custom capacities.
#![warn(missing_docs)]
#![cfg_attr(feature = "nightly", feature(alloc, read_initializer, specialization))]
#![cfg_attr(test, feature(test))]
#![cfg_attr(all(test, feature = "nightly"), feature(io))]

extern crate memchr;

extern crate safemem;

#[cfg(test)]
extern crate test;

use std::any::Any;
use std::io::prelude::*;
use std::io::SeekFrom;
use std::{cmp, error, fmt, io, ops, mem};

#[cfg(test)]
mod benches;

// ::std::io's tests require exact allocation which slice_deque cannot provide
#[cfg(test)]
mod std_tests;

#[cfg(all(test, feature = "slice-deque"))]
mod ringbuf_tests;

#[cfg(feature = "nightly")]
mod nightly;

#[cfg(feature = "nightly")]
use nightly::init_buffer;

mod buffer;

pub use buffer::{BufImpl, StdBuf};

#[cfg(feature = "slice-deque")]
pub use buffer::SliceDequeBuf;

pub mod policy;

use self::policy::{ReaderPolicy, WriterPolicy, StdPolicy, FlushOnNewline};

const DEFAULT_BUF_SIZE: usize = 8 * 1024;

/// A drop-in replacement for `std::io::BufReader` with more functionality.
///
/// Original method names/signatures and implemented traits are left untouched,
/// making replacement as simple as swapping the import of the type.
pub struct BufReader<R, P: ReaderPolicy = StdPolicy>{
    // First field for null pointer optimization.
    buf: Buffer,
    inner: R,
    policy: P,
}

impl<R> BufReader<R, StdPolicy> {
    /// Create a new `BufReader` wrapping `inner`, with a buffer of a
    /// default capacity and the default `ReadPolicy`.
    pub fn new(inner: R) -> Self {
        Self::with_capacity(DEFAULT_BUF_SIZE, inner)
    }

    /// Create a new `BufReader` wrapping `inner` with a capacity
    /// of *at least* `cap` bytes and the default `ReadPolicy`.
    ///
    /// The actual capacity of the buffer may vary based on
    /// implementation details of the buffer's allocator.
    pub fn with_capacity(cap: usize, inner: R) -> Self {
        Self::with_buffer(Buffer::with_capacity(cap), inner)
    }

    /// Allocate a buffer that never needs to move data to make room (consuming from the head
    /// immediately makes more room at the tail).
    ///
    /// This is useful in conjunction with `policy::MinBuffered` to ensure there is always room
    /// to read more data if necessary, without expensive copying operations.
    ///
    /// Only available on platforms with virtual memory support and with the `slice_deque` feature
    /// enabled. See `Buffer::with_capacity_ringbuf()` for more info.
    #[cfg(feature = "slice-deque")]
    pub fn new_ringbuf(inner: R) -> Self {
        Self::with_capacity_ringbuf(DEFAULT_BUF_SIZE, inner)
    }

    /// Allocate a buffer with the given capacity that never needs to move data to make room
    /// (consuming from the head immediately makes more room at the tail).
    ///
    /// This is useful in conjunction with `policy::MinBuffered` to ensure there is always room
    /// to read more data if necessary, without expensive copying operations.
    ///
    /// The capacity will be rounded up to the minimum size for the current platform (the next
    /// multiple of the page size, usually).
    ///
    /// Only available on platforms with virtual memory support and with the `slice_deque` feature
    /// enabled. See `Buffer::with_capacity_ringbuf()` for more info.
    #[cfg(feature = "slice-deque")]
    pub fn with_capacity_ringbuf(cap: usize, inner: R) -> Self {
        Self::with_buffer(Buffer::with_capacity_ringbuf(cap), inner)
    }

    /// Re-use an existing buffer with a new reader.
    ///
    /// ### Note
    /// Does **not** clear the buffer first! If there is data already in the buffer
    /// then it will be returned in `read()` and `fill_buf()`.
    pub fn with_buffer(buf: Buffer, inner: R) -> Self {
        BufReader {
            buf, inner, policy: StdPolicy
        }
    }
}

impl<R, P: ReaderPolicy> BufReader<R, P> {
    /// Apply a new `ReaderPolicy` to this `BufReader`, returning the transformed type.
    pub fn set_policy<P_: ReaderPolicy>(self, policy: P_) -> BufReader<R, P_> {
        BufReader { 
            inner: self.inner,
            buf: self.buf,
            policy
        }
    }

    /// Accessor for updating the `ReaderPolicy` in-place.
    ///
    /// If you want to change the type, use `.set_policy()`.
    pub fn policy_mut(&mut self) -> &mut P { &mut self.policy }

    /// Move data to the start of the buffer, making room at the end for more 
    /// reading.
    pub fn make_room(&mut self) {
        self.buf.make_room();        
    }

    /// Ensure room in the buffer for *at least* `additional` bytes. May not be
    /// quite exact due to implementation details of the buffer's allocator.
    pub fn reserve(&mut self, additional: usize) {
        self.buf.reserve(additional);
    }

    // RFC: pub fn shrink(&mut self, new_len: usize) ?

    /// Get the section of the buffer containing valid data; may be empty.
    ///
    /// Call `.consume()` to remove bytes from the beginning of this section.
    pub fn buffer(&self) -> &[u8] {
        self.buf.buf()
    }

    /// Get the current number of bytes available in the buffer.
    pub fn buf_len(&self) -> usize {
        self.buf.len()
    }

    /// Get the total buffer capacity.
    pub fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    /// Get an immutable reference to the underlying reader.
    pub fn get_ref(&self) -> &R { &self.inner }

    /// Get a mutable reference to the underlying reader.
    ///
    /// ## Note
    /// Reading directly from the underlying reader is not recommended, as some
    /// data has likely already been moved into the buffer.
    pub fn get_mut(&mut self) -> &mut R { &mut self.inner }

    /// Consume `self` and return the inner reader only.
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// Consume `self` and return both the underlying reader and the buffer.
    ///
    /// See also: `BufReader::unbuffer()`
    pub fn into_inner_with_buffer(self) -> (R, Buffer) {
        (self.inner, self.buf)
    }

    /// Consume `self` and return an adapter which implements `Read` and will
    /// empty the buffer before reading directly from the underlying reader.
    pub fn unbuffer(self) -> Unbuffer<R> {
        Unbuffer {
            inner: self.inner,
            buf: Some(self.buf),
        }
    }

    #[inline]
    fn should_read(&mut self) -> bool {
        self.policy.before_read(&mut self.buf).0
    }
}

impl<R: Read, P: ReaderPolicy> BufReader<R, P> {
    /// Unconditionally perform a read into the buffer.
    ///
    /// Does not invoke `ReaderPolicy` methods.
    /// 
    /// If the read was successful, returns the number of bytes read.
    pub fn read_into_buf(&mut self) -> io::Result<usize> {
        self.buf.read_from(&mut self.inner)
    }

    /// Box the inner reader without losing data.
    pub fn boxed<'a>(self) -> BufReader<Box<Read + 'a>, P> where R: 'a {
        let inner: Box<Read + 'a> = Box::new(self.inner);
        
        BufReader {
            inner,
            buf: self.buf,
            policy: self.policy,
        }
    }
}

impl<R: Read, P: ReaderPolicy> Read for BufReader<R, P> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        // If we don't have any buffered data and we're doing a read matching
        // or exceeding the internal buffer's capacity, bypass the buffer.
        if self.buf.is_empty() && out.len() >= self.buf.capacity() {
            return self.inner.read(out);
        }

        let nread = self.fill_buf()?.read(out)?;
        self.consume(nread);
        Ok(nread)
    }
}

impl<R: Read, P: ReaderPolicy> BufRead for BufReader<R, P> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        // If we've reached the end of our internal buffer then we need to fetch
        // some more data from the underlying reader.
        // This execution order is important; the policy may want to resize the buffer or move data
        // before reading into it.
        while self.should_read() && self.buf.usable_space() > 0 {
            if self.read_into_buf()? == 0 { break; };
        }

        Ok(self.buffer())
    }

    fn consume(&mut self, mut amt: usize) {
        amt = cmp::min(amt, self.buf_len());
        self.buf.consume(amt);
        self.policy.after_consume(&mut self.buf, amt);
    }
}

impl<R: fmt::Debug, P: ReaderPolicy + fmt::Debug> fmt::Debug for BufReader<R, P> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("buf_redux::BufReader")
            .field("reader", &self.inner)
            .field("buf_len", &self.buf_len())
            .field("capacity", &self.capacity())
            .field("policy", &self.policy)
            .finish()
    }
}

impl<R: Seek, P: ReaderPolicy> Seek for BufReader<R, P> {
    /// Seek to an ofPet, in bytes, in the underlying reader.
    ///
    /// The position used for seeking with `SeekFrom::Current(_)` is the
    /// position the underlying reader would be at if the `BufReader` had no
    /// internal buffer.
    ///
    /// Seeking always discards the internal buffer, even if the seek position
    /// would otherwise fall within it. This guarantees that calling
    /// `.unwrap()` immediately after a seek yields the underlying reader at
    /// the same position.
    ///
    /// See `std::io::Seek` for more details.
    ///
    /// Note: In the edge case where you're seeking with `SeekFrom::Current(n)`
    /// where `n` minus the internal buffer length underflows an `i64`, two
    /// seeks will be performed instead of one. If the second seek returns
    /// `Err`, the underlying reader will be left at the same position it would
    /// have if you seeked to `SeekFrom::Current(0)`.
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let result: u64;
        if let SeekFrom::Current(n) = pos {
            let remainder = self.buf_len() as i64;
            // it should be safe to assume that remainder fits within an i64 as the alternative
            // means we managed to allocate 8 ebibytes and that's absurd.
            // But it's not out of the realm of possibility for some weird underlying reader to
            // support seeking by i64::min_value() so we need to handle underflow when subtracting
            // remainder.
            if let Some(offset) = n.checked_sub(remainder) {
                result = self.inner.seek(SeekFrom::Current(offset))?;
            } else {
                // seek backwards by our remainder, and then by the offset
                self.inner.seek(SeekFrom::Current(-remainder))?;
                self.buf.clear(); // empty the buffer
                result = self.inner.seek(SeekFrom::Current(n))?;
            }
        } else {
            // Seeking with Start/End doesn't care about our buffer length.
            result = self.inner.seek(pos)?;
        }
        self.buf.clear();
        Ok(result)
    }
}

/// A type wrapping `Option` which provides more convenient access when the `Some` case is more
/// common.
struct AssertSome<T>(Option<T>);

impl<T> AssertSome<T> {
    fn new(val: T) -> Self {
        AssertSome(Some(val))
    }

    fn take(this: &mut Self) -> T {
        this.0.take().expect("Called `AssertSome::take()` more than once")
    }

    fn take_self(this: &mut Self) -> Self {
        AssertSome(this.0.take())
    }

    fn is_some(this: &Self) -> bool {
        this.0.is_some()
    }
}

const ASSERT_DEREF_ERR: &'static str = "Attempted to access value of AssertSome after calling \
                                        `AssertSome::take()`";

impl<T> ops::Deref for AssertSome<T> {
    type Target = T;
    
    fn deref(&self) -> &T { 
        self.0.as_ref().expect(ASSERT_DEREF_ERR)
    }
}

impl<T> ops::DerefMut for AssertSome<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.0.as_mut().expect(ASSERT_DEREF_ERR)
    }
}

/// A drop-in replacement for `std::io::BufWriter` with more functionality.
pub struct BufWriter<W: Write, P: WriterPolicy = StdPolicy> {
    buf: Buffer,
    inner: AssertSome<W>,
    policy: P,
    panicked: bool,
}

impl<W: Write> BufWriter<W> {
    /// Wrap `inner` with the default buffer capacity and flush strategy.
    pub fn new(inner: W) -> Self {
        Self::with_capacity(DEFAULT_BUF_SIZE, inner)
    }

    /// Wrap `inner` with the given buffer capacity and the default flush strategy.
    pub fn with_capacity(cap: usize, inner: W) -> Self {
        BufWriter {
            buf: Buffer::with_capacity(cap),
            inner: AssertSome::new(inner),
            policy: StdPolicy,
            panicked: false,
        }
    }
}

impl<W: Write, P: WriterPolicy> BufWriter<W, P> {
    /// Set a new `WriterPolicy`, returning the transformed type.
    pub fn set_policy<P_: WriterPolicy>(mut self, policy: P_) -> BufWriter<W, P_> {
        BufWriter {
            inner: AssertSome::take_self(&mut self.inner),
            buf: mem::replace(&mut self.buf, Buffer::with_capacity(0)),
            policy,
            panicked: self.panicked,
        }
    }

    /// Mutate the current `WriterPolicy`.
    pub fn policy_mut(&mut self) -> &mut P {
        &mut self.policy
    }

    /// Get a reference to the inner writer.
    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    /// Get a mutable reference to the inner writer.
    ///
    /// ### Note
    /// If the buffer has not been flushed, writing directly to the inner type will cause
    /// data inconsistency.
    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    /// Get the capacty of the inner buffer.
    pub fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    /// Get the number of bytes currently in the buffer.
    pub fn buf_len(&self) -> usize {
        self.buf.len()
    }

    /// Reserve space in the buffer for at least `additional` bytes. May not be
    /// quite exact due to implementation details of the buffer's allocator.
    pub fn reserve(&mut self, additional: usize) {
        self.buf.reserve(additional);
    }

    /// Flush the buffer and unwrap, returning the inner writer on success,
    /// or a type wrapping `self` plus the error otherwise.
    pub fn into_inner(mut self) -> Result<W, IntoInnerError<Self>> {
        match self.flush() {
            Err(e) => Err(IntoInnerError(self, e)),
            Ok(()) => Ok(AssertSome::take(&mut self.inner)),
        }
    }

    /// Flush the buffer and unwrap, returning the inner writer and
    /// any error encountered during flushing.
    pub fn into_inner_with_err(mut self) -> (W, Option<io::Error>) {
        let err = self.flush().err();
        (AssertSome::take(&mut self.inner), err)
    }

    /// Consume `self` and return both the underlying writer and the buffer.
    pub fn into_inner_with_buffer(mut self) -> (W, Buffer){
        (
            AssertSome::take(&mut self.inner),
            mem::replace(&mut self.buf, Buffer::with_capacity(0))
        )
    }

    fn flush_buf(&mut self, amt: usize) -> io::Result<()> {
        if amt == 0 || amt > self.buf.len() { return Ok(()) }

        self.panicked = true;
        let ret = self.buf.write_max(amt, &mut *self.inner);
        self.panicked = false;
        ret
    }
}

impl<W: Write, P: WriterPolicy> Write for BufWriter<W, P> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let flush_amt = self.policy.before_write(&mut self.buf, buf.len()).0;
        self.flush_buf(flush_amt)?;

        let written = if self.buf.is_empty() && buf.len() >= self.buf.capacity() {
            self.panicked = true;
            let result = self.inner.write(buf);
            self.panicked = false;
            result?
        } else {
            self.buf.copy_from_slice(buf)
        };

        let flush_amt = self.policy.after_write(&self.buf).0;

        let _ = self.flush_buf(flush_amt);

        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        let flush_amt = self.buf.len();
        self.flush_buf(flush_amt)?;
        self.inner.flush()
    }
}

impl<W: Write + Seek, P: WriterPolicy> Seek for BufWriter<W, P> {
    /// Seek to the ofPet, in bytes, in the underlying writer.
    ///
    /// Seeking always writes out the internal buffer before seeking.
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.flush().and_then(|_| self.get_mut().seek(pos))
    }
}

impl<W: fmt::Debug + Write, P: WriterPolicy + fmt::Debug> fmt::Debug for BufWriter<W, P> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("buf_redux::BufWriter")
            .field("writer", &*self.inner)
            .field("capacity", &self.capacity())
            .field("policy", &self.policy)
            .finish()
    }
}

impl<W: Write, P: WriterPolicy> Drop for BufWriter<W, P> {
    fn drop(&mut self) {
        if AssertSome::is_some(&self.inner) && !self.panicked {
            // dtors should not panic, so we ignore a failed flush
            let _r = self.flush();
        }
    }
}

/// A drop-in replacement for `std::io::LineWriter` with more functionality.
pub struct LineWriter<W: Write>(BufWriter<W, FlushOnNewline>);

impl<W: Write> LineWriter<W> {
    /// Wrap `inner` with the default buffer capacity.
    pub fn new(inner: W) -> Self {
        LineWriter(BufWriter::new(inner).set_policy(FlushOnNewline))
    }

    /// Wrap `inner` with the given buffer capacity.
    pub fn with_capacity(capacity: usize, inner: W) -> Self {
        LineWriter(BufWriter::with_capacity(capacity, inner).set_policy(FlushOnNewline))
    }

    /// Get a reference to the inner writer.
    pub fn get_ref(&self) -> &W {
        self.0.get_ref()
    }

    /// Get a mutable reference to the inner writer.
    ///
    /// ###Note
    /// If the buffer has not been flushed, writing directly to the inner type will cause
    /// data inconsistency.
    pub fn get_mut(&mut self) -> &mut W {
        self.0.get_mut()
    }

    /// Get the capacty of the inner buffer.
    pub fn capacity(&self) -> usize {
        self.0.capacity()
    }

    /// Get the number of bytes currently in the buffer.
    pub fn buf_len(&self) -> usize {
        self.0.buf_len()
    }

    /// Ensure enough space in the buffer for *at least* `additional` bytes. May not be
    /// quite exact due to implementation details of the buffer's allocator.
    pub fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional);
    }

    /// Flush the buffer and unwrap, returning the inner writer on success,
    /// or a type wrapping `self` plus the error otherwise.
    pub fn into_inner(self) -> Result<W, IntoInnerError<Self>> {
        self.0.into_inner()
            .map_err(|IntoInnerError(inner, e)| IntoInnerError(LineWriter(inner), e))
    }

    /// Flush the buffer and unwrap, returning the inner writer and
    /// any error encountered during flushing.
    pub fn into_inner_with_err(self) -> (W, Option<io::Error>) {
        self.0.into_inner_with_err()
    }

    /// Consume `self` and return both the underlying writer and the buffer.
    pub fn into_inner_with_buf(self) -> (W, Buffer){
        self.0.into_inner_with_buffer()
    }
}

impl<W: Write> Write for LineWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl<W: fmt::Debug + Write> fmt::Debug for LineWriter<W> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("buf_redux::LineWriter")
            .field("writer", self.get_ref())
            .field("capacity", &self.capacity())
            .finish()
    }
}

/// The error type for `BufWriter::into_inner()`,
/// contains the `BufWriter` as well as the error that occurred.
#[derive(Debug)]
pub struct IntoInnerError<W>(pub W, pub io::Error);

impl<W> IntoInnerError<W> {
    /// Get the error
    pub fn error(&self) -> &io::Error {
        &self.1
    }

    /// Take the writer.
    pub fn into_inner(self) -> W {
        self.0
    }
}

impl<W> Into<io::Error> for IntoInnerError<W> {
    fn into(self) -> io::Error {
        self.1
    }
}

impl<W: Any + Send + fmt::Debug> error::Error for IntoInnerError<W> {
    fn description(&self) -> &str {
        error::Error::description(self.error())
    }

    fn cause(&self) -> Option<&error::Error> {
        Some(&self.1)
    }
}

impl<W> fmt::Display for IntoInnerError<W> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.error().fmt(f)
    }
}

/// A deque-like datastructure for managing bytes.
///
/// Supports interacting via I/O traits like `Read` and `Write`, and direct access.
pub struct Buffer {
    buf: BufImpl,
    zeroed: usize,
}

impl Buffer {
    /// Create a new buffer with a default capacity.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_BUF_SIZE)
    }

    /// Create a new buffer with the given capacity.
    ///
    /// If the `Vec` ends up with extra capacity, `Buffer` will use all of it.
    pub fn with_capacity(cap: usize) -> Self {
        Buffer {
            buf: BufImpl::with_capacity(cap),
            zeroed: 0,
        }
    }

    /// Allocate a buffer that never needs to move data to make room (consuming from the head
    /// immediately makes more room at the tail).
    ///
    /// See `Buffer::with_capacity_ringbuf()` for more.
    #[cfg(feature = "slice-deque")]
    pub fn new_ringbuf() -> Self {
        Self::with_capacity_ringbuf(DEFAULT_BUF_SIZE)
    }

    /// Allocate a buffer with the given capacity that never needs to move data to make room
    /// (consuming from the head immediately makes more room at the tail).
    #[cfg(feature = "slice-deque")]
    pub fn with_capacity_ringbuf(cap: usize) -> Self {
        Buffer {
            buf: BufImpl::with_capacity_ringbuf(cap),
            zeroed: 0,
        }
    }

    /// Return the number of bytes currently in this buffer.
    ///
    /// Equivalent to `self.buf().len()`.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Return the number of bytes that can be read into this buffer before it needs
    /// to grow or the data in the buffer needs to be moved.
    ///
    /// This may not constitute all free space in the buffer if bytes have been consumed
    /// from the head. Use `free_space()` to determine the total free space in the buffer.
    pub fn usable_space(&self) -> usize {
        self.buf.usable_space()
    }

    /// Returns the total amount of free space in the buffer, including bytes
    /// already consumed from the head.
    ///
    /// This will be greater than or equal to `usable_space()`. On supported platforms
    /// with the `slice-deque` feature enabled, it should be equal.
    pub fn free_space(&self) -> usize {
        self.capacity() - self.len()
    }

    /// Return the total capacity of this buffer.
    pub fn capacity(&self) -> usize {
        self.buf.capacity()
    }

    /// Returns `true` if there are no bytes in the buffer, false otherwise.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Move bytes down in the buffer to maximize usable space.
    ///
    /// This is a no-op on supported platforms with the `slice-deque` feature enabled.
    pub fn make_room(&mut self) {
        self.buf.make_room();
    }

    /// Ensure space for at least `additional` more bytes in the buffer.
    ///
    /// This is a no-op if `usable_space() >= additional`. Note that this will reallocate
    /// even if there is enough free space at the head of the buffer for `additional` bytes.
    /// If you prefer copying data down in the buffer before attempting to reallocate you may wish
    /// to call `.make_room()` first.
    ///
    /// ### Panics
    /// If `self.capacity() + additional` overflows.
    pub fn reserve(&mut self, additional: usize) {
        // Returns `true` if we reallocated out-of-place and thus need to re-zero.
        if self.buf.reserve(additional) {
            self.zeroed = 0;
        }
    }

    /// Get an immutable slice of the available bytes in this buffer.
    ///
    /// Call `.consume()` to remove bytes from the beginning of this slice.
    pub fn buf(&self) -> &[u8] { self.buf.buf() }

    /// Get a mutable slice representing the available bytes in this buffer.
    ///
    /// Call `.consume()` to remove bytes from the beginning of this slice.
    pub fn buf_mut(&mut self) -> &mut [u8] { self.buf.buf_mut() }

    /// Read from `rdr`, returning the number of bytes read or any errors.
    ///
    /// If there is no more room at the head of the buffer, this will return `Ok(0)`.
    ///
    /// Uses `Read::initializer()` to initialize the buffer if the `nightly`
    /// feature is enabled, otherwise the buffer is zeroed if it has never been written.
    ///
    /// ###Panics
    /// If the returned count from `rdr.read()` overflows the tail cursor of this buffer.
    pub fn read_from<R: Read + ?Sized>(&mut self, rdr: &mut R) -> io::Result<usize> {
        if self.usable_space() == 0 {
            return Ok(0);
        }

        let cap = self.capacity();
        if self.zeroed < cap {
            unsafe {
                let buf = self.buf.write_buf();
                init_buffer(&rdr, buf);
            }

            self.zeroed = cap;
        }

        let read = {
            let mut buf = unsafe { self.buf.write_buf() };
            rdr.read(buf)?
        };

        unsafe {
            self.buf.bytes_written(read);
        }

        Ok(read)
    }

    /// Copy from `src` to the tail of this buffer. Returns the number of bytes copied.
    ///
    /// This will **not** grow the buffer if `src` is larger than `self.usable_space()`; instead,
    /// it will fill the usable space and return the number of bytes copied. If there is no usable
    /// space, this returns 0.
    pub fn copy_from_slice(&mut self, src: &[u8]) -> usize {
        let len = unsafe {
            let mut buf = self.buf.write_buf();
            let len = cmp::min(buf.len(), src.len());
            buf[..len].copy_from_slice(&src[..len]);
            len
        };

        unsafe {
            self.buf.bytes_written(len);
        }

        len
    }

    /// Write bytes from this buffer to `wrt`. Returns the number of bytes written or any errors.
    ///
    /// If the buffer is empty, returns `Ok(0)`.
    ///
    /// ### Panics
    /// If the count returned by `wrt.write()` would cause the head cursor to overflow or pass
    /// the tail cursor if added to it.
    pub fn write_to<W: Write + ?Sized>(&mut self, wrt: &mut W) -> io::Result<usize> {
        if self.len() == 0 {
            return Ok(0);
        }

        let written = wrt.write(self.buf())?;
        self.consume(written);
        Ok(written)
    }

    /// Write, at most, the given number of bytes from this buffer to `wrt`, continuing
    /// to write and ignoring interrupts, until the number is reached or the buffer is empty.
    ///
    /// ### Panics
    /// If the count returned by `wrt.write()` would cause the head cursor to overflow or pass
    /// the tail cursor if added to it.
    pub fn write_max<W: Write + ?Sized>(&mut self, mut max: usize, wrt: &mut W) -> io::Result<()> {
        while self.len() > 0 && max > 0 {
            let len = cmp::min(self.len(), max);
            let n = match wrt.write(&self.buf()[..len]) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero,
                                                   "Buffer::write_all() got zero-sized write")),
                Ok(n) => n,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };

            self.consume(n);
            max = max.saturating_sub(n);
        }

        Ok(())
    }

    /// Write all bytes in this buffer, ignoring interrupts. Continues writing until the buffer is
    /// empty or an error is returned.
    ///
    /// ### Panics
    /// If `self.write_to(wrt)` panics.
    pub fn write_all<W: Write + ?Sized>(&mut self, wrt: &mut W) -> io::Result<()> {
        while self.len() > 0 {
            match self.write_to(wrt) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero,
                                                   "Buffer::write_all() got zero-sized write")),
                Ok(_) => (),
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => (),
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    /// Copy bytes to `out` from this buffer, returning the number of bytes written.
    pub fn copy_to_slice(&mut self, out: &mut [u8]) -> usize {
        let len = {
            let buf = self.buf();

            let len = cmp::min(buf.len(), out.len());
            out[..len].copy_from_slice(&buf[..len]);
            len
        };

        self.consume(len);

        len
    }

    /// Push `bytes` to the end of the buffer, growing it if necessary.
    ///
    /// If you prefer moving bytes down in the buffer to reallocating, you may wish to call
    /// `.make_room()` first.
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        let s_len = bytes.len();

        if self.usable_space() < s_len {
            self.reserve(s_len * 2);
        }

        unsafe {
            self.buf.write_buf()[..s_len].copy_from_slice(bytes);
            self.buf.bytes_written(s_len);
        }
    }

    /// Consume `amt` bytes from the head of this buffer.
    pub fn consume(&mut self, amt: usize) {
        self.buf.consume(amt);
    }

    /// Empty this buffer by consuming all bytes.
    pub fn clear(&mut self) {
        let buf_len = self.len();
        self.consume(buf_len);
    }
}

impl fmt::Debug for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("buf_redux::Buffer")
            .field("capacity", &self.capacity())
            .field("len", &self.len())
            .finish()
    }
}

/// A `Read` adapter for a consumed `BufReader` which will empty bytes from the buffer before reading from
/// `inner` directly. Frees the buffer when it has been emptied. 
pub struct Unbuffer<R> {
    inner: R,
    buf: Option<Buffer>,
}

impl<R> Unbuffer<R> {
    /// Returns `true` if the buffer still has some bytes left, `false` otherwise.
    pub fn is_buf_empty(&self) -> bool {
        !self.buf.is_some()
    }

    /// Returns the number of bytes remaining in the buffer.
    pub fn buf_len(&self) -> usize {
        self.buf.as_ref().map(Buffer::len).unwrap_or(0)
    }

    /// Get a slice over the available bytes in the buffer.
    pub fn buf(&self) -> &[u8] {
        self.buf.as_ref().map_or(&[], Buffer::buf)
    }

    /// Return the underlying reader, releasing the buffer.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for Unbuffer<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if let Some(ref mut buf) = self.buf.as_mut() {
            let read = buf.copy_to_slice(out);

            if out.len() != 0 && read != 0 {
                return Ok(read);
            }
        }

        self.buf = None;

        self.inner.read(out)
    }
}

impl<R: fmt::Debug> fmt::Debug for Unbuffer<R> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("buf_redux::Unbuffer")
            .field("reader", &self.inner)
            .field("buffer", &self.buf)
            .finish()
    }
}

/// Copy data between a `BufRead` and a `Write` without an intermediate buffer.
///
/// Retries on interrupts.
pub fn copy_buf<B: BufRead, W: Write>(b: &mut B, w: &mut W) -> io::Result<u64> {
    let mut total_copied = 0;

    loop {
        let copied = match b.fill_buf().and_then(|buf| w.write(buf)) {
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
            Ok(buf) => buf,
        };

        if copied == 0 { break; }

        b.consume(copied);

        total_copied += copied as u64;
    }

    Ok(total_copied)
}

#[cfg(not(feature = "nightly"))]
fn init_buffer<R: Read + ?Sized>(_r: &R, buf: &mut [u8]) {
    // we can't trust a reader without nightly
    safemem::write_bytes(buf, 0);
}
