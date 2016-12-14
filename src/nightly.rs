// Copyright 2016 Austin Bonander <austin.bonander@gmail.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.
use std::fmt;
use std::io::{self, Read, Write};

use super::TrustRead;

use super::{BufReader, BufWriter, LineWriter};

use strategy::{FlushStrategy, MoveStrategy, ReadStrategy};

// ===== TrustRead impls =====
unsafe impl<R: Read> TrustRead for R {
    /// Default impl which always returns `false`.
    default fn is_trusted(&self) -> bool {
        false
    }
}

macro_rules! trust {
    ($($ty:path),+) => (
        $(unsafe impl $crate::TrustRead for $ty {
            /// Unconditional impl that returns `true`.
            fn is_trusted(&self) -> bool { true }
        })+
    )
}

trust! {
    ::std::io::Stdin, ::std::fs::File, ::std::net::TcpStream,
    ::std::io::Empty, ::std::io::Repeat
}

unsafe impl<'a> TrustRead for &'a [u8] {
    /// Unconditional impl that returns `true`.
    fn is_trusted(&self) -> bool { true }
}

unsafe impl<'a> TrustRead for ::std::io::StdinLock<'a> {
    /// Unconditional impl that returns `true`.
    fn is_trusted(&self) -> bool { true }
}

unsafe impl<T: AsRef<[u8]>> TrustRead for ::std::io::Cursor<T> {
    /// Unconditional impl that returns `true`.
    fn is_trusted(&self) -> bool { true }
}

unsafe impl<R: Read> TrustRead for ::std::io::BufReader<R> {
    /// Returns `self.get_ref().is_trusted()`
    fn is_trusted(&self) -> bool { self.get_ref().is_trusted() }
}

unsafe impl<R: Read, Rs: ReadStrategy, Ms: MoveStrategy> TrustRead for ::BufReader<R, Rs, Ms> {
    /// Returns `self.get_ref().is_trusted()`
    fn is_trusted(&self) -> bool { self.get_ref().is_trusted() }
}

/// A wrapper for a `Read` type that will unconditionally return `true` for `self.is_trusted()`.
///
/// See the `TrustRead` trait for more information.
pub struct AssertTrustRead<R>(R);

impl<R> AssertTrustRead<R> {
    /// Create a new `AssertTrustRead` wrapping `inner`.
    ///
    /// ###Safety
    /// Because this wrapper will return `true` for `self.is_trusted()`,
    /// the inner reader may be passed uninitialized memory containing potentially
    /// sensitive information from previous usage.
    ///
    /// Wrapping a reader with this type asserts that the reader will not attempt to access
    /// the memory passed to `Read::read()`.
    pub unsafe fn new(inner: R) -> Self {
        AssertTrustRead(inner)
    }

    /// Get a reference to the inner reader.
    pub fn get_ref(&self) -> &R { &self.0 }

    /// Get a mutable reference to the inner reader.
    ///
    /// Unlike `BufReader` (from this crate or the stdlib), calling `.read()` through this
    /// reference cannot cause logical inconsistencies because this wrapper does not take any
    /// data from the underlying reader.
    ///
    /// However, it is best if you use the I/O methods on this wrapper itself, especially with
    /// `BufReader` or `Buffer` as it allows them to elide zeroing of their buffers.
    pub fn get_mut(&mut self) -> &mut R { &mut self.0 }

    /// Take the wrapped reader by-value.
    pub fn into_inner(self) -> R { self.0 }
}

impl<R> AsRef<R> for AssertTrustRead<R> {
    fn as_ref(&self) -> &R { self.get_ref() }
}

impl<R> AsMut<R> for AssertTrustRead<R> {
    fn as_mut(&mut self) -> &mut R { self.get_mut() }
}

impl<R: Read> Read for AssertTrustRead<R> {
    /// Unconditionally calls through to `<R as Read>::read()`.
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.0.read(out)
    }
}

unsafe impl<R: Read> TrustRead for AssertTrustRead<R> {
    /// Unconditional impl that returns `true`.
    fn is_trusted(&self) -> bool { true }
}

impl<R, Rs: ReadStrategy, Ms: MoveStrategy> fmt::Debug for BufReader<R, Rs, Ms> {
    default fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("buf_redux::BufReader")
            .field("reader", &"(no Debug impl)")
            .field("available", &self.available())
            .field("capacity", &self.capacity())
            .field("read_strategy", &self.read_strat)
            .field("move_strategy", &self.move_strat)
            .finish()
    }
}

impl<W: Write, Fs: FlushStrategy> fmt::Debug for BufWriter<W, Fs> {
    default fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("buf_redux::BufWriter")
            .field("writer", &"(no Debug impl)")
            .field("capacity", &self.capacity())
            .field("flush_strategy", &self.flush_strat)
            .finish()
    }
}

impl<W: Write> fmt::Debug for LineWriter<W> {
    default fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("buf_redux::LineWriter")
            .field("writer", &"(no Debug impl)")
            .field("capacity", &self.capacity())
            .finish()
    }
}