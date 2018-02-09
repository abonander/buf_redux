// Original implementation Copyright 2013 The Rust Project Developers <https://github.com/rust-lang>
//
// Original source file: https://github.com/rust-lang/rust/blob/master/src/libstd/io/buffered.rs
//
// Additions copyright 2016 Austin Bonander <austin.bonander@gmail.com>
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
#![cfg_attr(feature = "nightly", feature(alloc, specialization))]
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

#[cfg(test)]
mod std_tests;

#[cfg(test)]
mod tests;

#[cfg(feature = "nightly")]
mod nightly;

mod raw;

pub mod strategy;

use self::strategy::{
    MoveStrategy, DefaultMoveStrategy,
    ReadStrategy, DefaultReadStrategy,
    FlushStrategy, DefaultFlushStrategy,
    FlushOnNewline
};

use self::raw::RawBuf;

const DEFAULT_BUF_SIZE: usize = 8 * 1024;

/// A drop-in replacement for `std::io::BufReader` with more functionality.
///
/// Original method names/signatures and implemented traits are left untouched,
/// making replacement as simple as swapping the import of the type.
pub struct BufReader<R, Rs = DefaultReadStrategy, Ms = DefaultMoveStrategy>{
    // First field for null pointer optimization.
    buf: Buffer,
    inner: R,
    read_strat: Rs,
    move_strat: Ms,
}

impl<R> BufReader<R, DefaultReadStrategy, DefaultMoveStrategy> {
    /// Create a new `BufReader` wrapping `inner`, with a buffer of a
    /// default capacity and the default strategies.
    pub fn new(inner: R) -> Self {
        Self::with_strategies(inner, Default::default(), Default::default())
    }

    /// Create a new `BufReader` wrapping `inner` with a capacity
    /// of *at least* `cap` bytes and the default strategies.
    ///
    /// The actual capacity of the buffer may vary based on
    /// implementation details of the buffer's allocator.
    pub fn with_capacity(cap: usize, inner: R) -> Self {
        Self::with_cap_and_strategies(inner, cap, Default::default(), Default::default())
    }
}

impl<R, Rs: ReadStrategy, Ms: MoveStrategy> BufReader<R, Rs, Ms> {
    /// Create a new `BufReader` wrapping `inner`, with a default buffer capacity
    /// and with the given `ReadStrategy` and `MoveStrategy`.
    pub fn with_strategies(inner: R, rs: Rs, ms: Ms) -> Self {
        Self::with_cap_and_strategies(inner, DEFAULT_BUF_SIZE, rs, ms)
    }

    /// Create a new `BufReader` wrapping `inner`, with a buffer capacity of *at least*
    /// `cap` bytes and the given `ReadStrategy` and `MoveStrategy`.
    /// 
    /// The actual capacity of the buffer may vary based on
    /// implementation details of the buffer's allocator.
    pub fn with_cap_and_strategies(inner: R, cap: usize, rs: Rs, ms: Ms) -> Self {
        BufReader {
            inner: inner,
            buf: Buffer::with_capacity(cap),
            read_strat: rs,
            move_strat: ms,
        }
    }

    /// Apply a new `MoveStrategy` to this `BufReader`, returning the transformed type.
    pub fn move_strategy<Ms_: MoveStrategy>(self, ms: Ms_) -> BufReader<R, Rs, Ms_> {
        BufReader { 
            inner: self.inner,
            buf: self.buf,
            read_strat: self.read_strat,
            move_strat: ms,
        }
    }

    /// Apply a new `ReadStrategy` to this `BufReader`, returning the transformed type.
    pub fn read_strategy<Rs_: ReadStrategy>(self, rs: Rs_) -> BufReader<R, Rs_, Ms> {
        BufReader { 
            inner: self.inner,
            buf: self.buf,
            read_strat: rs,
            move_strat: self.move_strat,
        }
    }

    /// Accessor for updating the `MoveStrategy` in-place.
    ///
    /// If you want to change the type, use `.move_strategy()`.
    pub fn move_strategy_mut(&mut self) -> &mut Ms { &mut self.move_strat }

    /// Accessor for updating the `ReadStrategy` in-place.
    ///
    /// If you want to change the type, use `.read_strategy()`.
    pub fn read_strategy_mut(&mut self) -> &mut Rs { &mut self.read_strat }

    /// Move data to the start of the buffer, making room at the end for more 
    /// reading.
    pub fn make_room(&mut self) {
        self.buf.make_room();        
    }

    /// Grow the internal buffer by *at least* `additional` bytes. May not be
    /// quite exact due to implementation details of the buffer's allocator.
    /// 
    /// ##Note
    /// This should not be called frequently as each call will incur a 
    /// reallocation.
    pub fn grow(&mut self, additional: usize) {
        self.buf.grow(additional);
    }

    // RFC: pub fn shrink(&mut self, new_len: usize) ?

    /// Get the section of the buffer containing valid data; may be empty.
    ///
    /// Call `.consume()` to remove bytes from the beginning of this section.
    pub fn get_buf(&self) -> &[u8] {
        self.buf.buf()
    }

    /// Get the current number of bytes available in the buffer.
    pub fn available(&self) -> usize {
        self.buf.buffered()
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

    /// Consume `self` and return both the underlying reader and the buffer,
    /// with the data moved to the beginning and the length truncated to contain
    /// only valid data.
    ///
    /// See also: `BufReader::unbuffer()`
    pub fn into_inner_with_buf(self) -> (R, Vec<u8>) {
        (self.inner, self.buf.into_inner())
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
    fn should_read(&self) -> bool {
        self.read_strat.should_read(&self.buf)
    }

    #[inline]
    fn should_move(&self) -> bool {
        self.move_strat.should_move(&self.buf)
    }
}

impl<R: Read, Rs: ReadStrategy, Ms: MoveStrategy> BufReader<R, Rs, Ms> {
    /// Unconditionally perform a read into the buffer, calling `.make_room()`
    /// if appropriate or necessary, as determined by the implementation.
    ///
    /// If the read was successful, returns the number of bytes read.
    pub fn read_into_buf(&mut self) -> io::Result<usize> { 
        if self.should_move() {
            self.make_room();
        }
        
        self.buf.read_from(&mut self.inner)
    }
}

impl<R: Read, Rs, Ms> BufReader<R, Rs, Ms> {
    /// Box the inner reader without losing data.
    pub fn boxed<'a>(self) -> BufReader<Box<Read + 'a>, Rs, Ms> where R: 'a {
        let inner: Box<Read + 'a> = Box::new(self.inner);
        
        BufReader {
            inner: inner,
            buf: self.buf,
            read_strat: self.read_strat,
            move_strat: self.move_strat,
        }
    }
}

impl<R: Read, Rs: ReadStrategy, Ms: MoveStrategy> Read for BufReader<R, Rs, Ms> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        // If we don't have any buffered data and we're doing a massive read
        // (larger than our internal buffer), bypass our internal buffer
        // entirely.
        if self.buf.is_empty() && out.len() > self.buf.capacity() {
            return self.inner.read(out);
        }

        let nread = {
            let mut rem = try!(self.fill_buf());
            try!(rem.read(out))
        };
        self.consume(nread);
        Ok(nread)
    }
}

impl<R: Read, Rs: ReadStrategy, Ms: MoveStrategy> BufRead for BufReader<R, Rs, Ms> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        // If we've reached the end of our internal buffer then we need to fetch
        // some more data from the underlying reader.
        if self.should_read() {
            let _ = try!(self.read_into_buf());
        }

        Ok(self.get_buf())
    }

    fn consume(&mut self, amt: usize) {
        self.buf.consume(amt);
    }
}

impl<R: fmt::Debug, Rs: ReadStrategy, Ms: MoveStrategy> fmt::Debug for BufReader<R, Rs, Ms> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("buf_redux::BufReader")
            .field("reader", &self.inner)
            .field("available", &self.available())
            .field("capacity", &self.capacity())
            .field("read_strategy", &self.read_strat)
            .field("move_strategy", &self.move_strat)
            .finish()
    }
}

impl<R: Seek, Rs: ReadStrategy, Ms: MoveStrategy> Seek for BufReader<R, Rs, Ms> {
    /// Seek to an offset, in bytes, in the underlying reader.
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
            let remainder = self.available() as i64;
            // it should be safe to assume that remainder fits within an i64 as the alternative
            // means we managed to allocate 8 ebibytes and that's absurd.
            // But it's not out of the realm of possibility for some weird underlying reader to
            // support seeking by i64::min_value() so we need to handle underflow when subtracting
            // remainder.
            if let Some(offset) = n.checked_sub(remainder) {
                result = try!(self.inner.seek(SeekFrom::Current(offset)));
            } else {
                // seek backwards by our remainder, and then by the offset
                try!(self.inner.seek(SeekFrom::Current(-remainder)));
                self.buf.clear(); // empty the buffer
                result = try!(self.inner.seek(SeekFrom::Current(n)));
            }
        } else {
            // Seeking with Start/End doesn't care about our buffer length.
            result = try!(self.inner.seek(pos));
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
        this.0.take().expect("Called AssertSome::take() more than once")
    }

    fn take_self(this: &mut Self) -> Self {
        AssertSome(this.0.take())
    }

    fn is_some(this: &Self) -> bool {
        this.0.is_some()
    }
}

const ASSERT_DEREF_ERR: &'static str = "Attempt to access value of AssertSome after calling AssertSome::take()";

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
pub struct BufWriter<W: Write, Fs: FlushStrategy = DefaultFlushStrategy> {
    buf: Buffer,
    inner: AssertSome<W>,
    flush_strat: Fs,
    panicked: bool,
}

impl<W: Write> BufWriter<W, DefaultFlushStrategy> {
    /// Wrap `inner` with the default buffer capacity and flush strategy.
    pub fn new(inner: W) -> Self {
        Self::with_strategy(inner, Default::default())
    }

    /// Wrap `inner` with the given buffer capacity and the default flush strategy.
    pub fn with_capacity(capacity: usize, inner: W) -> Self {
        Self::with_capacity_and_strategy(capacity, inner, Default::default())
    }
}

impl<W: Write, Fs: FlushStrategy> BufWriter<W, Fs> {
    /// Wrap `inner` with the default buffer capacity and given flush strategy
    pub fn with_strategy(inner: W, flush_strat: Fs) -> Self {
        Self::with_capacity_and_strategy(DEFAULT_BUF_SIZE, inner, flush_strat)
    }

    /// Wrap `inner` with the given buffer capacity and flush strategy.
    pub fn with_capacity_and_strategy(capacity: usize, inner: W, flush_strat: Fs) -> Self {
        BufWriter {
            inner: AssertSome::new(inner),
            buf: Buffer::with_capacity(capacity),
            flush_strat: flush_strat,
            panicked: false,
        }
    }

    /// Set a new `FlushStrategy`, returning the transformed type.
    pub fn flush_strategy<Fs_: FlushStrategy>(mut self, flush_strat: Fs_) -> BufWriter<W, Fs_> {
        BufWriter {
            inner: AssertSome::take_self(&mut self.inner),
            buf: mem::replace(&mut self.buf, Buffer::with_capacity(0)),
            flush_strat: flush_strat,
            panicked: self.panicked,
        }
    }

    /// Mutate the current flush strategy.
    pub fn flush_strategy_mut(&mut self) -> &mut Fs {
        &mut self.flush_strat
    }

    /// Get a reference to the inner writer.
    pub fn get_ref(&self) -> &W {
        &self.inner
    }

    /// Get a mutable reference to the inner writer.
    ///
    /// ###Note
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
    pub fn buffered(&self) -> usize {
        self.buf.buffered()
    }

    /// Grow the internal buffer by *at least* `additional` bytes. May not be
    /// quite exact due to implementation details of the buffer's allocator.
    /// 
    /// ##Note
    /// This should not be called frequently as each call will incur a 
    /// reallocation.
    pub fn grow(&mut self, additional: usize) {
        self.buf.grow(additional);
    }

    /// Flush the buffer and unwrap, returning the inner writer on success,
    /// or a type wrapping `self` plus the error otherwise.
    pub fn into_inner(mut self) -> Result<W, IntoInnerError<Self>> {
        match self.flush_buf() {
            Err(e) => Err(IntoInnerError(self, e)),
            Ok(()) => Ok(AssertSome::take(&mut self.inner)),
        }
    }

    /// Flush the buffer and unwrap, returning the inner writer and
    /// any error encountered during flushing.
    pub fn into_inner_with_err(mut self) -> (W, Option<io::Error>) {
        let err = self.flush_buf().err();
        (AssertSome::take(&mut self.inner), err)
    }

    /// Consume `self` and return both the underlying writer and the buffer,
    /// with the data moved to the beginning and the length truncated to contain
    /// only valid data.
    pub fn into_inner_with_buf(mut self) -> (W, Vec<u8>){
        (
            AssertSome::take(&mut self.inner),
            mem::replace(&mut self.buf, Buffer::with_capacity(0)).into_inner()
        )
    }

    fn flush_buf(&mut self) -> io::Result<()> {
        self.panicked = true;
        let ret = self.buf.write_all(&mut *self.inner);
        self.panicked = false;
        ret
    }
}

impl<W: Write, Fs: FlushStrategy> Write for BufWriter<W, Fs> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.flush_strat.flush_before(&self.buf, buf.len()) {
            try!(self.flush_buf());
        }

        if buf.len() >= self.buf.capacity() {
            self.panicked = true;
            let ret = self.inner.write(buf);
            self.panicked = false;
            ret
        } else {
            Ok(self.buf.copy_from_slice(buf))
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        try!(self.flush_buf());
        self.inner.flush()
    }
}

impl<W: Write + Seek, Fs: FlushStrategy> Seek for BufWriter<W, Fs> {
    /// Seek to the offset, in bytes, in the underlying writer.
    ///
    /// Seeking always writes out the internal buffer before seeking.
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.flush_buf().and_then(|_| self.get_mut().seek(pos))
    }
}

impl<W: fmt::Debug + Write, Fs: FlushStrategy> fmt::Debug for BufWriter<W, Fs> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("buf_redux::BufWriter")
            .field("writer", &*self.inner)
            .field("capacity", &self.capacity())
            .field("flush_strategy", &self.flush_strat)
            .finish()
    }
}

impl<W: Write, Fs: FlushStrategy> Drop for BufWriter<W, Fs> {
    fn drop(&mut self) {
        if AssertSome::is_some(&self.inner) && !self.panicked {
            // dtors should not panic, so we ignore a failed flush
            let _r = self.flush_buf();
        }
    }
}

/// A drop-in replacement for `std::io::LineWriter` with more functionality.
pub struct LineWriter<W: Write>(BufWriter<W, FlushOnNewline>);

impl<W: Write> LineWriter<W> {
    /// Wrap `inner` with the default buffer capacity.
    pub fn new(inner: W) -> Self {
        LineWriter(BufWriter::with_strategy(inner, FlushOnNewline))
    }

    /// Wrap `inner` with the given buffer capacity.
    pub fn with_capacity(capacity: usize, inner: W) -> Self {
        LineWriter(BufWriter::with_capacity_and_strategy(capacity, inner, FlushOnNewline))
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
    pub fn buffered(&self) -> usize {
        self.0.buffered()
    }

    /// Grow the internal buffer by *at least* `additional` bytes. May not be
    /// quite exact due to implementation details of the buffer's allocator.
    ///
    /// ##Note
    /// This should not be called frequently as each call will incur a
    /// reallocation.
    pub fn grow(&mut self, additional: usize) {
        self.0.grow(additional);
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

    /// Consume `self` and return both the underlying writer and the buffer,
    /// with the data moved to the beginning and the length truncated to contain
    /// only valid data.
    pub fn into_inner_with_buf(self) -> (W, Vec<u8>){
        self.0.into_inner_with_buf()
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
    buf: RawBuf,
    pos: usize,
    end: usize,
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
            buf: RawBuf::with_capacity(cap),
            pos: 0,
            end: 0,
            zeroed: 0,
        }
    }

    /// Return the number of bytes currently in this buffer.
    ///
    /// Equivalent to `self.buf().len()`.
    pub fn buffered(&self) -> usize {
        self.end - self.pos
    }

    /// Return the number of bytes that can be read into this buffer before it needs
    /// to grow or the data in the buffer needs to be moved.
    pub fn headroom(&self) -> usize {
        self.buf.len() - self.end
    }

    /// Return the total capacity of this buffer.
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Returns `true` if there are no bytes in the buffer, false otherwise.
    pub fn is_empty(&self) -> bool {
        self.buffered() == 0
    }

    /// Grow the buffer by `additional` bytes.
    ///
    /// ###Panics
    /// If `self.capacity() + additional` overflows.
    pub fn grow(&mut self, additional: usize) { 
        self.check_cursors();

        // Returns `false` if we reallocated out-of-place and thus need to re-zero.
        if !self.buf.resize(self.end, additional) {
            self.zeroed = 0;
        }
    }

    /// Reset the cursors if there is no data remaining.
    ///
    /// Returns true if there is no more potential headroom.
    fn check_cursors(&mut self) -> bool {
        if self.pos == 0 {
            true
        } else if self.pos == self.end {
            self.pos = 0;
            self.end = 0;
            true
        } else {
            false
        }
    }

    /// Make room in the buffer, moving data down to the beginning if necessary.
    ///
    /// Does not grow the buffer or delete unread bytes from it.
    pub fn make_room(&mut self) {
        if self.check_cursors() {
            return;
        }

        let copy_amt = self.buffered();
        // Guaranteed lowering to memmove.
        safemem::copy_over(self.buf.get_mut(), self.pos, 0, copy_amt);

        self.end -= self.pos;
        self.pos = 0;
    }            

    /// Get an immutable slice of the available bytes in this buffer.
    ///
    /// Call `.consume()` to remove bytes from the beginning of this slice.
    pub fn buf(&self) -> &[u8] { self.buf.slice(self.pos .. self.end) }

    /// Get a mutable slice representing the available bytes in this buffer.
    ///
    /// Call `.consume()` to remove bytes from the beginning of this slice.
    pub fn buf_mut(&mut self) -> &mut [u8] { self.buf.slice_mut(self.pos .. self.end) }

    /// Read from `rdr`, returning the number of bytes read or any errors.
    ///
    /// If there is no more room at the head of the buffer, this will return `Ok(0)`.
    ///
    /// If `<R as TrustRead>::is_trusted(rdr)` returns `true`,
    /// this method can avoid zeroing the head of the buffer.
    ///
    /// See the `TrustRead` trait for more information.
    ///
    /// ###Panics
    /// If the returned count from `rdr.read()` overflows the head cursor of this buffer.
    pub fn read_from<R: Read + ?Sized>(&mut self, rdr: &mut R) -> io::Result<usize> {
        self.check_cursors();

        if self.headroom() == 0 {
            return Ok(0);
        }

        if !rdr.is_trusted() && self.zeroed < self.buf.len() {
            let start = cmp::max(self.end, self.zeroed);

            safemem::write_bytes(self.buf.slice_mut(start..), 0);

            self.zeroed = self.buf.len();
        }

        let read = try!(rdr.read(self.buf.slice_mut(self.end..)));

        let new_end = self.end.checked_add(read).expect("Overflow adding bytes read to self.end");

        self.end = cmp::min(self.buf.len(), new_end);

        Ok(read)
    }

    /// Copy from `src` to the head of this buffer. Returns the number of bytes copied.
    ///
    /// This will **not** grow the buffer if `src` is larger than `self.headroom()`; instead,
    /// it will fill the headroom and return the number of bytes copied. If there is no headroom,
    /// this returns 0.
    pub fn copy_from_slice(&mut self, src: &[u8]) -> usize {
        self.check_cursors();
    
        let len = cmp::min(self.buf.len() - self.end, src.len());

        self.buf.slice_mut(self.end .. self.end + len).copy_from_slice(&src[..len]);

        self.end += len;

        len
    }

    /// Write bytes from this buffer to `wrt`. Returns the number of bytes written or any errors.
    ///
    /// If the buffer is empty, returns `Ok(0)`.
    ///
    /// ###Panics
    /// If the count returned by `wrt.write()` would overflow the tail cursor if added to it.
    pub fn write_to<W: Write + ?Sized>(&mut self, wrt: &mut W) -> io::Result<usize> {
        self.check_cursors();

        if self.buf.len() == 0 {
            return Ok(0);
        }

        let written = try!(wrt.write(self.buf()));

        let new_pos = self.pos.checked_add(written)
            .expect("Overflow adding bytes written to self.pos");

        self.pos = cmp::min(new_pos, self.end);
        Ok(written)
    }

    /// Write all bytes in this buffer, ignoring interrupts. Continues writing until the buffer is
    /// empty or an error is returned.
    ///
    /// ###Panics
    /// If `self.write_to(wrt)` panics.
    pub fn write_all<W: Write + ?Sized>(&mut self, wrt: &mut W) -> io::Result<()> {
        while self.buffered() > 0 {
            match self.write_to(wrt) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "Buffer::write_all() got zero-sized write")),
                Ok(_) => (),
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => (),
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    /// Copy bytes to `out` from this buffer, returning the number of bytes written.
    pub fn copy_to_slice(&mut self, out: &mut [u8]) -> usize {
        self.check_cursors();

        let len = {
            let buf = self.buf();

            let len = cmp::min(buf.len(), out.len());
            out[..len].copy_from_slice(&buf[..len]);
            len
        };

        self.pos += len;

        len
    }

    /// Push `bytes` to the end of the buffer, growing it if necessary.
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        self.check_cursors();

        let s_len = bytes.len();

        if self.headroom() < s_len {
            self.grow(s_len * 2);
        }

        self.buf.slice_mut(s_len..).copy_from_slice(bytes);
        self.end += s_len;
    }

    /// Consume `amt` bytes from the tail of this buffer. No more than `self.available()` bytes
    /// will be consumed.
    pub fn consume(&mut self, amt: usize) {
        let avail = self.buffered();

        if amt >= avail {
            self.clear();
        } else {
            self.pos += amt;
        }
    }

    /// Empty this buffer by resetting the cursors.
    pub fn clear(&mut self) {
        self.pos = 0;
        self.end = 0;
    }

    /// Move the bytes down the beginning of the buffer and take the inner vector, truncated
    /// to the number of bytes available.
    pub fn into_inner(mut self) -> Vec<u8> {
        self.make_room();
        let avail = self.buffered();
        let mut buf = self.buf.into_vec();
        buf.truncate(avail);
        buf
    }
}

impl fmt::Debug for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("buf_redux::Buffer")
            .field("capacity", &self.capacity())
            .field("available", &self.buffered())
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
        self.buf.as_ref().map(Buffer::buffered).unwrap_or(0)
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

/// A trait which `Buffer` can use to determine whether or not
/// it is safe to elide zeroing of its buffer.
///
/// Has a default implementation of `is_trusted()` which always returns `false`.
///
/// Use the `nightly` feature to enable specialization, which means this
/// trait can be implemented for specifically trusted types from the stdlib
/// and potentially elsewhere.
///
/// ###Motivation
/// As part of its intended operation, `Buffer` can pass a potentially
/// uninitialized slice of its buffer to `Read::read()`. Untrusted readers could access sensitive
/// information in this slice, from previous usage of that region of memory,
/// which has not been overwritten yet. Thus, the uninitialized parts of the buffer need to be zeroed
/// to prevent unintentional leakage of information.
///
/// However, for trusted readers which are known to only write to this slice and not read from it,
/// such as various types in the stdlib which will pass the slice directly to a syscall,
/// this zeroing is an unnecessary waste of cycles which the optimizer may or may not elide properly.
///
/// This trait helps `Buffer` determine whether or not a particular reader is trustworthy.
pub unsafe trait TrustRead: Read {
    /// Return `true` if this reader does not need a zeroed slice passed to `.read()`.
    fn is_trusted(&self) -> bool;
}

#[cfg(not(feature = "nightly"))]
unsafe impl<R: Read> TrustRead for R {
    /// Default impl which always returns `false`.
    ///
    /// Enable the `nightly` feature to specialize this impl for various types.
    fn is_trusted(&self) -> bool {
        false
    }
}

#[cfg(feature = "nightly")]
pub use nightly::AssertTrustRead;

