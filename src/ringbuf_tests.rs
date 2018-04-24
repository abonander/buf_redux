// Copyright 2018 Austin Bonander <austin.bonander@gmail.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Tests checking `Buffer::new_ringbuf()` and friends.

use super::{Buffer, DEFAULT_BUF_SIZE};

macro_rules! assert_capacity {
    ($buf:expr, $cap:expr) => {
        let cap = $buf.capacity();
            if cfg!(windows) {
            // Windows' minimum allocation size is 64K
            assert_eq!(cap, ::std::cmp::max(64 * 1024, cap));
        } else {
            assert_eq!(cap, $cap);
        }
    }
}

#[test]
fn test_buffer_new() {
    let buf = Buffer::new_ringbuf();
    assert_capacity!(buf, DEFAULT_BUF_SIZE);
}

#[test]
fn test_buffer_with_cap() {
    let buf = Buffer::with_capacity_ringbuf(4 * 1024);
    assert_capacity!(buf, DEFAULT_BUF_SIZE);

    // test rounding up to page size
    let buf = Buffer::with_capacity_ringbuf(64);
    assert_capacity!(buf, 4 * 1024);
}
