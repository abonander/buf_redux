// Copyright 2016 Austin Bonander <austin.bonander@gmail.com>
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use super::Buffer;

use std::io::Cursor;

#[test]
fn read_into_full() {
    let mut buffer = Buffer::with_capacity(1);

    assert_eq!(buffer.capacity(), 1);

    let mut bytes = Cursor::new([1u8, 2]);

    // Result<usize, io::Error> does not impl PartialEq
    assert_eq!(buffer.read_from(&mut bytes).unwrap(), 1);
    assert_eq!(buffer.read_from(&mut bytes).unwrap(), 0);
}