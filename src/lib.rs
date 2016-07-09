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
#![warn(missing_docs)]
#![cfg_attr(feature = "nightly", feature(io))]

use std::io::prelude::*;
use std::io::SeekFrom;
use std::{cmp, fmt, io, mem, ptr};

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
pub struct BufReader<R, Rs = DefaultReadStrategy, Ms = DefaultMoveStrategy>{
    inner: R,
    buf: Buffer,
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
            buf: Buffer::new(),
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

    /// Accessor for updating the `MoveStrategy` in-place. Must be the same type.
    pub fn move_strategy_mut(&mut self) -> &mut Ms { &mut self.move_strat }

    /// Accessor for updating the `ReadStrategy` in-place. Must be the same type.
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
    /// reallocation and a zeroing of the new memory.
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
        self.buf.available()
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

    /// Consumes `self` and returns the inner reader only.
    pub fn into_inner(self) -> R {
        self.inner
    }

    /// Consumes `self` and returns both the underlying reader and the buffer, 
    /// with the data moved to the beginning and the length truncated to contain
    /// only valid data.
    ///
    /// See also: `BufReader::unbuffer()`
    pub fn into_inner_with_buf(self) -> (R, Vec<u8>) {
        (self.inner, self.buf.into_inner())
    }

    /// Consumes `self` and returns an adapter which implements `Read` and will 
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

    fn should_move(&self) -> bool {
        self.move_strat.should_move(&self.buf)
    }
}

impl<R: Read, Rs: ReadStrategy, Ms: MoveStrategy> BufReader<R, Rs, Ms> {
    /// Unconditionally perform a read into the buffer, calling `.make_room()`
    /// if appropriate or necessary, as determined by the implementation.
    ///
    /// If the read was successful, returns the number of bytes now available 
    /// in the buffer.
    pub fn read_into_buf(&mut self) -> io::Result<usize> { 
        if self.should_move() {
            self.make_room();
        }
        
        self.buf.read_from(&mut self.inner)
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

pub struct Buffer {
    buf: Vec<u8>,
    pos: usize,
    end: usize,
}

impl Buffer {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_BUF_SIZE)
    }

    pub fn with_capacity(cap: usize) -> Self {
        let mut buf = vec![0; cap];
        let cap = buf.capacity();
        buf.resize(cap, 0);

        Buffer {
            buf: buf,
            pos: 0,
            end: 0,
        }
    }

    pub unsafe fn with_capacity_unzeroed(cap: usize) -> Self {
        let mut buf = Vec::with_capacity(cap);
        let cap = buf.capacity();
        buf.set_len(cap);

        Buffer {
            buf: buf,
            pos: 0,
            end: 0,
        }
    }

    pub fn available(&self) -> usize {
        self.end - self.pos
    }

    pub fn headroom(&self) -> usize {
        self.buf.len() - self.end
    }

    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.available() == 0
    }

    pub unsafe fn grow_unzeroed(&mut self, additional: usize) {
        self.check_cursors();

        let new_len = self.buf.len().checked_add(additional)
            .expect("Overflow calculating new capacity");

        let buf = mem::replace(&mut self.buf, Vec::with_capacity(new_len));
        let cap = self.buf.capacity();
            
        self.buf.set_len(cap);

        // WTB smarter borrowing for temporaries
        let avail = self.available();

        self.buf[.. avail]
            .copy_from_slice(&buf[self.pos .. self.end]);

        self.end -= self.pos;
        self.pos = 0;
    }

    pub fn grow(&mut self, additional: usize) {
        self.check_cursors();

        let new_len = self.buf.len().checked_add(additional)
            .expect("Overflow calculating new capacity");

        let buf = mem::replace(&mut self.buf, vec![0; new_len]);

        let cap = self.buf.capacity();
        self.buf.resize(cap, 0);

        let avail = self.available();

        self.buf[.. avail]
            .copy_from_slice(&buf[self.pos .. self.end]);

        self.end -= self.pos;
        self.pos = 0;
    }

    fn check_cursors(&mut self) -> bool {
        if self.pos == 0 {
            false
        } else if self.pos == self.end {
            self.pos = 0;
            self.end = 0;
            false
        } else {
            true
        }
    }

    pub fn make_room(&mut self) {
        if self.check_cursors() {
            return;
        }
        
        let src = self.buf[self.pos..].as_ptr();
        let dest = self.buf.as_mut_ptr();

        // Guaranteed lowering to memmove.
        // FIXME: Replace with a safe implementation when one becomes available.
        unsafe {
            ptr::copy(src, dest, self.end - self.pos);
        }

        self.end -= self.pos;
        self.pos = 0;

    }            

    pub fn buf(&self) -> &[u8] { &self.buf[self.pos .. self.end] }

    pub fn buf_mut(&mut self) -> &mut [u8] { &mut self.buf[self.pos .. self.end] }

    pub fn read_from<R: Read>(&mut self, rdr: &mut R) -> io::Result<usize> {
        let _ = self.check_cursors();

        let read = try!(rdr.read(&mut self.buf[self.end..]));
        self.end += cmp::min(self.buf.len(), read);
        Ok(read)
    }

    pub fn copy_from_slice(&mut self, src: &[u8]) -> usize {
        let _ = self.check_cursors();
    
        let len = cmp::min(self.buf.len() - self.end, src.len());

        self.buf[self.end..].copy_from_slice(&src[..len]);

        self.end += len;

        len
    }

    pub fn write_to(&mut self, wrt: &mut Write) -> io::Result<usize> {
        let _ = self.check_cursors();

        let written = try!(wrt.write(self.buf()));
        self.pos += cmp::min(written, self.end);
        Ok(written)
    }

    pub fn copy_to_slice(&mut self, out: &mut [u8]) -> usize {
        let _ = self.check_cursors();

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
        let _ = self.check_cursors();

        let s_len = bytes.len();

        if self.headroom() < s_len {
            self.grow(s_len * 2);
        }

        self.buf[s_len..].copy_from_slice(bytes);
        self.end += s_len;
    }

    pub fn consume(&mut self, amt: usize) {
        let avail = self.available();

        if amt == avail {
            self.clear();
        } else {
            let amt = cmp::min(self.available(), amt);
            self.pos += amt;
        }
    }

    pub fn clear(&mut self) {
        self.pos = 0;
        self.end = 0;
    }

    pub fn into_inner(mut self) -> Vec<u8> {
        self.make_room();
        let avail = self.available();
        self.buf.truncate(avail);
        self.buf
    }
}

impl Read for Buffer {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        Ok(self.copy_to_slice(out))
    }
}

impl Write for Buffer {
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        Ok(self.copy_from_slice(src))
    }

    fn write_all(&mut self, src: &[u8]) -> io::Result<()> {
        self.push_bytes(src);
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl fmt::Debug for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("buf_redux::Buffer")
            .field("capacity", &self.capacity())
            .field("available", &self.available())
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
        self.buf.as_ref().map(Buffer::available).unwrap_or(0)
    }

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
            let read = buf.read(out).unwrap();

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

