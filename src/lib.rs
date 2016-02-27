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
//! A drop-in replacement for `std::io::BufReader` with more functionality.
//!
//! Method names/signatures and implemented traits are unchanged from `std::io::BufReader`, making replacement as simple as swapping the import of the type:
//!
//! ```notest
//! - use std::io::BufReader;
//! + extern crate buf_redux;
//! + use buf_redux::BufReader;
//! ```
//! ### More Direct Control
//!
//! Provides methods to:
//!
//! * Access the buffer through an `&`-reference without performing I/O
//! * Force unconditional reads into the buffer
//! * Shuffle bytes down to the beginning of the buffer to make room for more reading
//! * Increase the capacity of the buffer
//! * Get the number of available bytes as well as the total capacity of the buffer
//! * Consume the `BufReader` without losing data
//! * Get inner reader and trimmed buffer with the remaining data
//! * Get a `Read` adapter which empties the buffer and then pulls from the inner reader directly
//! *
//!
//! ### More Sensible and Customizable Buffering Behavior
//! * Tune the behavior of the buffer to your specific use-case using the types in the [`strategy`
//! module](strategy/index.html):
//!     * `BufReader` performs reads as dictated by the [`ReadStrategy`
//!     trait](strategy/trait.ReadStrategy.html).
//!     * `BufReader` shuffles bytes down to the beginning of the buffer, to make more room at the end, when deemed appropriate by the
//! [`MoveStrategy` trait](strategy/trait.MoveStrategy.html).
//! * `BufReader` uses exact allocation instead of leaving it up to `Vec`, which allocates sizes in powers of two.
//!     * Vec's behavior is more efficient for frequent growth, but much too greedy for infrequent growth and custom capacities.
//!
//! See the `BufReader` type in this crate for more info.

#![cfg_attr(feature = "nightly", feature(io))]

use std::io::prelude::*;
use std::io::SeekFrom;
use std::{cmp, fmt, io, ptr};

#[cfg(test)]
mod tests;

pub mod strategy;

use self::strategy::{MoveStrategy, ReadStrategy, IfEmpty, AtEndLessThan1k};

const DEFAULT_BUF_SIZE: usize = 8 * 1024;

pub type DefaultReadStrategy = IfEmpty;
pub type DefaultMoveStrategy = AtEndLessThan1k;

/// The *pièce de résistance:* a drop-in replacement for `std::io::BufReader` with more functionality.
///
/// Original method names/signatures and implemented traits are left untouched,
/// making replacement as simple as swapping the import of the type.
pub struct BufReader<R, Rs, Ms>{
    inner: R,
    buf: Vec<u8>,
    pos: usize,
    cap: usize,
    read_strat: Rs, 
    move_strat: Ms,
}

impl<R> BufReader<R, DefaultReadStrategy, DefaultMoveStrategy> {
    /// Create a new `BufReader` wrapping `inner`, with a buffer of a
    /// default capacity and default strategies.
    pub fn new(inner: R) -> Self {
        Self::with_strategies(inner, Default::default(), Default::default())
    }

    /// Create a new `BufReader` wrapping `inner` with a capacity
    /// of *at least* `cap` bytes and default strategies.
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
        let mut self_ = BufReader { 
            inner: inner,
            buf: Vec::new(),
            pos: 0,
            cap: 0,
            read_strat: rs,
            move_strat: ms,
        };

        self_.grow(cap);
        self_
    }

    /// Apply a new `MoveStrategy` to this `BufReader`, returning the transformed type.
    pub fn move_strategy<Ms_: MoveStrategy>(self, ms: Ms_) -> BufReader<R, Rs, Ms_> {
        BufReader { 
            inner: self.inner,
            buf: self.buf,
            pos: self.pos,
            cap: self.cap,
            read_strat: self.read_strat,
            move_strat: ms,
        }
    }

    /// Apply a new `ReadStrategy` to this `BufReader`, returning the transformed type.
    pub fn read_strategy<Rs_: ReadStrategy>(self, rs: Rs_) -> BufReader<R, Rs_, Ms> {
        BufReader { 
            inner: self.inner,
            buf: self.buf,
            pos: self.pos,
            cap: self.cap,
            read_strat: rs,
            move_strat: self.move_strat,
        }
    }

    /// Accessor for updating the `MoveStrategy` in-place. Must be the same type.
    pub fn move_strategy_mut(&mut self) -> &mut Ms { &mut self.move_strat }

    /// Accessor for updating the `ReadStrategy` in-place. Must be the same type.
    pub fn read_strategy_mut(&mut self) -> &mut Rs { &mut self.read_strat }

    /// Move data to the start of the buffer, making room at the end for more 
    /// reading.
    pub fn make_room(&mut self) {
        if self.pos == 0 {
            return;
        }

        if self.pos == self.cap {
            self.pos = 0;
            self.cap = 0;
            return;
        }

        let src = self.buf[self.pos..].as_ptr();
        let dest = self.buf.as_mut_ptr();

        // Guaranteed lowering to memmove.
        // FIXME: Replace with a safe implementation when one becomes available.
        unsafe {
            ptr::copy(src, dest, self.cap - self.pos);
        }

        self.cap -= self.pos;
        self.pos = 0;
    }

    /// Grow the internal buffer by *at least* `additional` bytes. May not be
    /// quite exact due to implementation details of the buffer's allocator.
    /// 
    /// ##Note
    /// This should not be called frequently as each call will incur a 
    /// reallocation and a zeroing of the new memory.
    pub fn grow(&mut self, additional: usize) {
        // We're not expecting to grow frequently, so the power-of-two growth
        // of `Vec::reserve()` is unnecessarily greedy.
        self.buf.reserve_exact(additional);

        // According to reserve_exact(), the allocator can still return more 
        // memory than requested; if that's the case, we might as well 
        // use all of it.
        let new_len = self.buf.capacity();
        self.buf.resize(new_len, 0u8);
    }

    // RFC: pub fn shrink(&mut self, new_len: usize) ?

    /// Get the section of the buffer containing valid data; may be empty.
    ///
    /// Call `.consume()` to remove bytes from the beginning of this section.
    pub fn get_buf(&self) -> &[u8] {
        &self.buf[self.pos .. self.cap]
    }

    /// Get the current number of bytes available in the buffer.
    pub fn available(&self) -> usize {
        self.cap - self.pos
    }

    /// Get the total buffer capacity.
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Get an immutable reference to the underlying reader.
    pub fn get_ref(&self) -> &R { &self.inner }

    /// Get a mutable reference to the underlying reader.
    ///
    /// ## Note
    /// Reading directly from the underlying reader is not recommended, as some
    /// data has likely already been moved into the buffer.
    pub fn get_mut(&mut self) -> &mut R { &mut self.inner }

    /// Consumes `self` and returns the inner reader only.
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// Consumes `self` and returns both the underlying reader and the buffer, 
    /// with the data moved to the beginning and the length truncated to contain
    /// only valid data.
    ///
    /// See also: `BufReader::unbuffer()`
    pub fn into_inner_with_buf(mut self) -> (R, Vec<u8>) {
        self.make_room();
        self.buf.truncate(self.cap);
        (self.inner, self.buf)
    }

    /// Consumes `self` and returns an adapter which implements `Read` and will 
    /// empty the buffer before reading directly from the underlying reader.
    pub fn unbuffer(mut self) -> Unbuffer<R> {
        self.buf.truncate(self.cap);

        Unbuffer {
            inner: self.inner,
            buf: self.buf,
            pos: self.pos,
        }
    }

    #[inline]
    fn should_read(&self) -> bool {
        self.read_strat.should_read(self.pos, self.cap, self.buf.len())
    }

    fn should_move(&self) -> bool {
        self.move_strat.should_move(self.pos, self.cap, self.buf.len())
    }
}

impl<R: Read, Rs: ReadStrategy, Ms: MoveStrategy> BufReader<R, Rs, Ms> {
    /// Unconditionally perform a read into the buffer, calling `.make_room()`
    /// if appropriate or necessary, as determined by the implementation.
    ///
    /// If the read was successful, returns the number of bytes now available 
    /// in the buffer.
    pub fn read_into_buf(&mut self) -> io::Result<usize> {
        if self.pos == self.cap {
            self.cap = try!(self.inner.read(&mut self.buf));
            self.pos = 0;
        } else { 
            if self.should_move() {
                self.make_room();
            }

            self.cap += try!(self.inner.read(&mut self.buf[self.cap..]));
        }

        Ok(self.cap - self.pos)
    }
}

impl<R: Read, Rs: ReadStrategy, Ms: MoveStrategy> Read for BufReader<R, Rs, Ms> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // If we don't have any buffered data and we're doing a massive read
        // (larger than our internal buffer), bypass our internal buffer
        // entirely.
        if self.pos == self.cap && buf.len() >= self.buf.len() {
            return self.inner.read(buf);
        }

        let nread = {
            let mut rem = try!(self.fill_buf());
            try!(rem.read(buf))
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

        Ok(&self.buf[self.pos..self.cap])
    }

    fn consume(&mut self, amt: usize) {
        self.pos = cmp::min(self.pos + amt, self.cap);
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
            let remainder = (self.cap - self.pos) as i64;
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
                self.pos = self.cap; // empty the buffer
                result = try!(self.inner.seek(SeekFrom::Current(n)));
            }
        } else {
            // Seeking with Start/End doesn't care about our buffer length.
            result = try!(self.inner.seek(pos));
        }
        self.pos = self.cap; // empty the buffer
        Ok(result)
    }
}

/// A `Read` adapter for a consumed `BufReader` which will empty bytes from the buffer before reading from
/// `inner` directly. Frees the buffer when it has been emptied. 
pub struct Unbuffer<R> {
    inner: R,
    buf: Vec<u8>,
    pos: usize,
}

impl<R> Unbuffer<R> {
    /// Returns `true` if the buffer still has some bytes left, `false` otherwise.
    pub fn is_buf_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }

    /// Returns the number of bytes remaining in the buffer.
    pub fn buf_len(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    /// Return the underlying reader, finally letting the buffer die in peace and join its family
    /// in allocation-heaven.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for Unbuffer<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos < self.buf.len() {
            // RFC: Is `try!()` necessary here since this shouldn't
            // really return an error, ever?
            let read = try!((&self.buf[self.pos..]).read(buf));
            self.pos += read;

            if self.pos == self.buf.len() {
                self.buf == Vec::new();
            }

            Ok(read)
        } else {
            self.inner.read(buf)
        }
    }
}

impl<R: fmt::Debug> fmt::Debug for Unbuffer<R> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("buf_redux::Unbuffer")
            .field("reader", &self.inner)
            .field("buffer", &format_args!("{}/{}", self.pos, self.buf.len()))
            .finish()
    }
}

// RFC: impl<R: BufRead> BufRead for Unbuffer<R> ?
