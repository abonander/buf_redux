// Copyright 2018 Austin Bonander <austin.bonander@gmail.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms
//! Move-free buffer and reader utilizing the [`slice-deque`] crate.
//!
//! These types are only available on target platforms with virtual memory support,
//! namely Windows, OS X and Linux.
//!
//! [`slice-deque`]: https://crates.io/crates/slice-deque
extern crate slice_deque;

pub use slice_deque::SliceDeque;

pub struct Buffer {
    deque: SliceDeque<u8>,
    zeroed: usize,
}

impl Buffer {
    pub fn new() -> Self {
        Self::with_capacity(super::DEFAULT_BUF_SIZE)
    }

    pub fn with_capacity(cap: usize) -> Self {
        Buffer {
            deque: SliceDeque::with_capacity(cap),
            zeroed: 0,
        }
    }

    /// Return the number of bytes currently in this buffer.
    ///
    /// Equivalent to `self.buf().len()`.
    pub fn buffered(&self) -> usize {
        self.deque.len()
    }

    /// Return the number of bytes that can be read into this buffer before it needs
    /// to grow or the data in the buffer needs to be moved.
    pub fn headroom(&self) -> usize {
        self.capacity() - self.buffered()
    }

    /// Return the total capacity of this buffer.
    pub fn capacity(&self) -> usize {
        self.deque.capacity()
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
        self.deque.reserve(additional)
    }

    /// Get an immutable slice of the available bytes in this buffer.
    ///
    /// Call `.consume()` to remove bytes from the beginning of this slice.
    pub fn buf(&self) -> &[u8] { &self.deque }

    /// Get a mutable slice representing the available bytes in this buffer.
    ///
    /// Call `.consume()` to remove bytes from the beginning of this slice.
    pub fn buf_mut(&mut self) -> &mut [u8] { &mut self.deque }

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
        if self.headroom() == 0 {
            return Ok(0);
        }

        let mut buf = unsafe { self.deque.tail_head_slice() };

        if self.zeroed < self.capacity() {
            init_buffer(buf);
            self.zeroed = self.capacity();
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
        self.deque.truncate(0)
    }

    /// Move the bytes down the beginning of the buffer and take the inner vector, truncated
    /// to the number of bytes available.
    pub fn into_inner(self) -> SliceDeque<u8> {
        self.deque
    }
}