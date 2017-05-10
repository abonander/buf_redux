// Copyright 2016 Austin Bonander <austin.bonander@gmail.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

pub use self::impl_::RawBuf;

#[cfg(not(feature = "nightly"))]
mod impl_ {
    use std::ops::{Index, IndexMut};

    pub struct RawBuf {
        buf: Vec<u8>,
    }

    impl RawBuf {
        pub fn with_capacity(capacity: usize) -> Self {
            let mut buf = Vec::with_capacity(capacity);
            let true_cap = buf.capacity();

            unsafe {
                buf.set_len(true_cap);
            }

            RawBuf {
                buf: buf
            }
        }

        pub fn len(&self) -> usize {
            self.buf.capacity()
        }

        pub fn get_mut(&mut self) -> &mut [u8] {
            &mut self.buf
        }

        pub fn slice<R>(&self, range: R) -> &<[u8] as Index<R>>::Output
        where [u8]: Index<R> {
            &(*self.buf)[range]
        }

        pub fn slice_mut<R>(&mut self, range: R) -> &mut <[u8] as Index<R>>::Output
        where [u8]: IndexMut<R> {
            &mut (*self.buf)[range]
        }

        pub fn resize(&mut self, used: usize, additional: usize) -> bool {
            let cap = self.buf.capacity();

            assert!(used <= cap, "Cannot have used more than current capacity");

            let old_ptr = self.buf.as_ptr();

            if used == 0 {
                *self = RawBuf::with_capacity(
                    cap.checked_add(additional)
                        .expect("overflow evalutating additional capacity")
                );

                return false;
            }

            self.buf.reserve_exact(additional);

            unsafe {
                let new_cap = self.len();
                self.buf.set_len(new_cap);
            }

            old_ptr == self.buf.as_ptr()
        }

        pub fn into_vec(self) -> Vec<u8> {
            self.buf
        }
    }
}

#[cfg(feature = "nightly")]
mod impl_ {
    extern crate alloc;

    use self::alloc::raw_vec::RawVec;

    use std::slice;
    use std::ops::{Index, IndexMut};

    pub struct RawBuf {
        buf: RawVec<u8>,
    }

    impl RawBuf {
        pub fn with_capacity(capacity: usize) -> Self {
            RawBuf {
                buf: RawVec::with_capacity(capacity)
            }
        }

        pub fn len(&self) -> usize {
            self.buf.cap()
        }

        pub fn get(&self) -> &[u8] {
            unsafe {
                slice::from_raw_parts(self.buf.ptr(), self.len())
            }
        }

        pub fn get_mut(&mut self) -> &mut [u8] {
            unsafe {
                slice::from_raw_parts_mut(self.buf.ptr(), self.len())
            }
        }

        pub fn slice<R>(&self, range: R) -> &<[u8] as Index<R>>::Output
        where [u8]: Index<R> {
            &self.get()[range]
        }

        pub fn slice_mut<R>(&mut self, range: R) -> &mut <[u8] as Index<R>>::Output
        where [u8]: IndexMut<R> {
            &mut self.get_mut()[range]
        }

        pub fn resize(&mut self, used: usize, additional: usize) -> bool {
            let cap = self.len();

            assert!(used <= cap, "Cannot have used more than current capacity");

            if !self.buf.reserve_in_place(cap, additional) {
                let old_ptr = self.buf.ptr();

                if used == 0 {
                    // Free the old buf and alloc a new one so the allocator doesn't
                    // bother copying bytes we no longer care about
                    self.buf = RawVec::with_capacity(
                        cap.checked_add(additional)
                            .expect("Overflow evaluating additional capacity")
                    );
                } else {
                    self.buf.reserve_exact(cap, additional);
                }

                return old_ptr == self.buf.ptr();
            }

            true
        }

        pub fn into_vec(self) -> Vec<u8> {
            unsafe {
                self.buf.into_box().into_vec()
            }
        }
    }
}
