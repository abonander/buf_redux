# buf\_re(a)dux

Drop-in replacements for buffered I/O types in `std::io`.

These replacements retain the method names/signatures and implemented traits of their stdlib
counterparts, making replacement as simple as swapping the import of the type.

### More Direct Control

All replacement types provide methods to:
* Increase the capacity of the buffer
* Get the number of available bytes as well as the total capacity of the buffer
* Consume the wrapper without losing data

`BufReader` provides methods to:
* Access the buffer through an `&`-reference without performing I/O
* Force unconditional reads into the buffer
* Get a `Read` adapter which empties the buffer and then pulls from the inner reader directly
* Shuffle bytes down to the beginning of the buffer to make room for more reading
* Get inner reader and trimmed buffer with the remaining data

`BufWriter` and `LineWriter` provide methods to:
* Flush the buffer and unwrap the inner writer unconditionally.

### More Sensible and Customizable Buffering Behavior
* Tune the behavior of the buffer to your specific use-case using the types in the `strategy`
module:
    * `BufReader` performs reads as dictated by the `ReadStrategy` trait.
    * `BufReader` moves bytes down to the beginning of the buffer, to make more room at the end, when deemed appropriate by the
`MoveStrategy` trait.
    * `BufWriter` flushes bytes to the inner writer when full, or when deemed appropriate by
        the `FlushStrategy` trait.
* `Buffer` uses exact allocation instead of leaving it up to `Vec`, which allocates sizes in powers of two.
    * Vec's behavior is more efficient for frequent growth, but much too greedy for infrequent growth and custom capacities.

## Usage

####[Documentation](http://docs.rs/buf_redux/)

`Cargo.toml`:
```toml
[dependencies]
buf_redux = "0.2"
```

`lib.rs` or `main.rs`:
```rust
extern crate buf_redux;
```

And then simply swap the import of the types you want to replace:

#### `BufReader`:
```
- use std::io::BufReader;
+ use buf_redux::BufReader;
```
#### `BufWriter`:
```
- use std::io::BufWriter;
+ use buf_redux::BufWriter;
```

#### `LineWriter`:
```
- use std::io::LineWriter;
+ use buf_redux::LineWriter;
```

## License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
