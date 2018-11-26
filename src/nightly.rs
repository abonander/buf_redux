// Copyright 2016-2018 Austin Bonander <austin.bonander@gmail.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Anything requiring unstable features (specialization, `Read::initializer()`, etc)

use std::fmt;
use std::io::{Read, Write};

use super::{BufReader, BufWriter, LineWriter, Unbuffer};

/// Wrapper specializing for `Debug` or defaulting to a string.
///
/// Specializing `Debug` impls with two type params would equire 4 `impl` blocks
/// if it were even possible, but that also requires lattice impls.
///
/// Instead, we specialize one impl per type and then in that impl also use specialization.
struct SpecialDebug<T> {
    val: T,
    default: &'static str,
}

impl<T> fmt::Debug for SpecialDebug<T> {
    default fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.default)
    }
}

impl<T: fmt::Debug> fmt::Debug for SpecialDebug<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        T::fmt(&self.val, f)
    }
}

impl<R, P> fmt::Debug for BufReader<R, P> {
    default fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("buf_redux::BufReader")
            .field("reader", &SpecialDebug { val: &self.inner, default: "(impl std::io::Read)" })
            .field("buf_len", &self.buf_len())
            .field("capacity", &self.capacity())
            .field("policy",
                   &SpecialDebug { val: &self.policy, default: ("(impl buf_redux::ReaderPolicy)")})
            .finish()
    }
}

impl<W: Write, P> fmt::Debug for BufWriter<W, P> {
    default fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("buf_redux::BufWriter")
            .field("writer", &SpecialDebug { val: &self.inner, default: "(impl std::io::Write)"})
            .field("capacity", &self.capacity())
            .field("policy",
                   &SpecialDebug { val: &self.policy, default: "(impl buf_redux::WriterPolicy)"})
            .finish()
    }
}

// vanilla specializations
impl<W: Write> fmt::Debug for LineWriter<W> {
    default fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("buf_redux::LineWriter")
            .field("writer", &"(impl std::io::Write)")
            .field("capacity", &self.capacity())
            .finish()
    }
}

impl<R> fmt::Debug for Unbuffer<R> {
    default fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("buf_redux::Unbuffer")
            .field("reader", &"(impl std::io::Read)")
            .field("buffer", &self.buf)
            .finish()
    }
}

pub fn init_buffer<R: Read + ?Sized>(rdr: &R, buf: &mut [u8]) {
    // no invariants for consumers to uphold:
    // https://doc.rust-lang.org/nightly/std/io/trait.Read.html#method.initializer
    unsafe { rdr.initializer().initialize(buf) }
}
