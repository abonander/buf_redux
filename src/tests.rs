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