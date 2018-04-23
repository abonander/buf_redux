// Copyright 2018 Austin Bonander <austin.bonander@gmail.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

mod std_buf;

#[cfg(feature = "slice-deque")]
mod slice_deque_buf;

pub use self::std_buf::StdBuf;

#[cfg(feature = "slice-deque")]
pub use self::slice_deque_buf::SliceDequeBuf;

/// Operations needed from a backing-buffer implementation.
///
/// The buffer is assumed to operate like a double-ended queue.
pub trait BufImpl: Sized {
    /// Allocate a new backing-buffer with the given capacity.
    ///
    /// The buffer is allowed to overallocate if it prefers but it should make this capacity
    /// available.
    fn with_capacity(cap: usize) -> Self;

    /// Return the number of bytes this buffer can hold in total.
    fn capacity(&self) -> usize;

    /// Return the number of bytes this buffer currently holds.
    ///
    /// **Must** be equivalent to `self.buf().len()`.
    fn len(&self) -> usize;

    /// Return the amount of space available for writing into the buffer.
    ///
    /// **Must** be equivalent to `self.write_buf().len()`.
    fn usable_space(&self) -> usize;

    /// Increase the buffer's size enough to accommodate an additional number of bytes.
    ///
    /// Return `true` if the buffer had to reallocate in a different memory location,
    /// false if the reallocation occurred in-place.
    fn reserve(&mut self, additional: usize) -> bool;

    /// If supported, increase the buffer's size enough to accommodate an additional number of bytes
    /// without moving the allocation.
    ///
    /// If successful, return `true`; if a moving reallocation needs to be done or this operation
    /// is not supported, return `false`.
    fn reserve_in_place(&mut self, additional: usize) -> bool { false }

    /// Do any sort of cleanup/moving necessary to regain wasted space in the buffer.
    fn make_room(&mut self);

    /// Return a view into the occupied portion of the buffer.
    fn buf(&self) -> &[u8];

    /// Return a mutable view into the occupied portion of the buffer.
    fn buf_mut(&mut self) -> &mut [u8];

    /// Return a mutable view into the unoccupied/uninitialized portion of the buffer.
    ///
    /// ### Unsafety
    /// This method can return a view into uninitialized data. Consumers should only write to this
    /// slice, not read from it.
    unsafe fn write_buf(&mut self) -> &mut [u8];

    /// Extend the tail of the safe/occupied portion of the buffer by this many bytes.
    ///
    /// Called after data is written into the buffer returned by `write_buf()`.
    ///
    /// As a sanity check, this should panic if the number of additional bytes exceeds the capacity
    /// of the buffer.
    ///
    /// ### Unsafety
    /// The buffer may assume that the additional number of bytes were written to the head
    /// of `write_buf()` and so it is safe to read them back.
    unsafe fn bytes_written(&mut self, add: usize);

    /// Consume/drop the given number of bytes from the head of the buffer.
    ///
    /// May clamp the amount to `self.len()` or panic if its value is exceeded.
    fn consume(&mut self, amt: usize);
}
