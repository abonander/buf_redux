// Copyright 2016-2018 Austin Bonander <austin.bonander@gmail.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
//! Types which can be used to tune the behavior of `BufReader`.
//!
//! Some simple strategies are provided for your convenience. You may prefer to create your own
//! types and implement the traits for them instead.

use super::Buffer;

use std::{fmt, ops};

/// Flag for `ReaderPolicy` methods to signal whether or not `BufReader` should read into the buffer.
///
/// See `do_read!()` for a shorthand.
#[derive(Copy, Clone, Debug)]
pub struct DoRead(pub bool);

/// Shorthand for invoking `DoRead(bool)` or `DoRead(true)` (empty invocation)
#[macro_export]
macro_rules! do_read (
    ($val:expr) => ( return $crate::policy::DoRead($val); );
    () => ( do_read!(true); )
);

/// Both a `ReaderPolicy` and a `WriterPolicy` that reproduces the behaviors for `std::io::BufReader`
/// and `std::io::BufWriter`, respectively.
#[derive(Debug, Default)]
pub struct StdPolicy;

/// Trait that governs `BufReader`'s behavior.
pub trait ReaderPolicy: Default + fmt::Debug {
    /// Consulted before attempting to read into the buffer.
    ///
    /// Return `DoRead(true)` to issue a read into the buffer
    /// before reading data out of it, or `DoRead(false)` to read from the buffer as it is,
    /// even if it's empty. `do_read!()` is provided as a shorthand.
    ///
    /// If there is no room in the buffer after this method is called,
    /// the buffer will not be read into (so if the buffer is full but you want more data
    /// you should resize it or call `.make_room()`). If there *is* room, `BufReader` will attempt
    /// to read into the buffer. If successful (`Ok(x)` where `x > 0` is returned), this method will
    /// be consulted again for another read attempt.
    ///
    /// ### Note
    /// If the read will ignore the buffer entirely (if the buffer is empty and the amount to be
    /// read matches or exceeds its capacity) or if `BufReader::read_into_buf()` was called to force
    /// a read into the buffer manually, this method will not be called.
    fn before_read(&mut self, buffer: &mut Buffer) -> DoRead;

    /// Called after bytes are consumed from the buffer.
    ///
    /// Supplies the true amount consumed if the amount passed to `BufReader::consume`
    /// was in excess.
    fn after_consume(&mut self, buffer: &mut Buffer, amt: usize) {}
}

/// Behavior of `std::io::BufReader`: the buffer will only be read into if it is empty.
impl ReaderPolicy for StdPolicy {
    fn before_read(&mut self, buffer: &mut Buffer) -> DoRead {
        DoRead(buffer.buffered() == 0)
    }
}

/// A `ReaderPolicy` which ensures there are at least this many bytes in the buffer,
/// failing this only if the reader is at EOF.
#[derive(Debug)]
pub struct MinBuffered(pub usize);

impl MinBuffered {
    pub fn set_min(&mut self, min: usize) {
        self.0 = min;
    }
}

impl ReaderPolicy for MinBuffered {
    fn before_read(&mut self, buffer: &mut Buffer) -> DoRead {
        min_buffered_impl(buffer, self.0)
    }
}

/// A trait which tells `BufWriter` when to flush.
pub trait WriterPolicy: Default + fmt::Debug {
    /// Return `true` if the buffer should be flushed before reading into it.
    ///
    /// The buffer is provided, as well as `incoming` which is
    /// the size of the buffer that will be written to the `BufWriter`.
    fn flush_before(&self, _buf: &Buffer, _incoming: usize) -> bool;

    /// Return `true` if the buffer should be flushed after reading into it.
    ///
    /// `buf` references the updated buffer after the read.
    ///
    /// Default impl is a no-op.
    fn flush_after(&self, _buf: &Buffer) -> bool {
        false
    }
}

/// Default behavior of `std::io::BufWriter`: flush before a read into the buffer
/// only if the incoming data is larger than the buffer's headroom.
impl WriterPolicy for StdPolicy {
    fn flush_before(&self, buf: &Buffer, incoming: usize) -> bool {
        buf.headroom() <= incoming
    }
}

/// Flush the buffer if it contains at least the given number of bytes.
#[derive(Debug, Default)]
pub struct FlushAtLeast(pub usize);

impl WriterPolicy for FlushAtLeast {
    fn flush_after(&self, buf: &Buffer, _: usize) -> bool {
        buf.buffered() > self.0
    }
}

/// Flush the buffer if it contains the given byte.
///
/// Only scans the buffer after reading. Searches from the end first.
#[derive(Debug, Default)]
pub struct FlushOn(pub u8);

impl WriterPolicy for FlushOn {
    /// Same as `StdPolicy`.
    fn flush_before(&self, buf: &Buffer, incoming: usize) -> bool {
        buf.headroom() < incoming
    }

    fn flush_after(&self, buf: &Buffer) -> bool {
        ::memchr::memrchr(self.0, buf.buf()).is_some()
    }
}

/// Flush the buffer if it contains a newline (`\n`).
///
/// Equivalent to `FlushOn(b'\n')`.
#[derive(Debug, Default)]
pub struct FlushOnNewline;

impl WriterPolicy for FlushOnNewline {
    /// Same as `StdPolicy`.
    fn flush_before(&self, buf: &Buffer, incoming: usize) -> bool {
        buf.headroom() < incoming
    }

    fn flush_after(&self, buf: &Buffer) -> bool {
        ::memchr::memrchr(b'\n', buf.buf()).is_some()
    }
}
