# buf\_re(a)dux

A drop-in replacement for Rust's `std::io::BufReader` with additional functionality. 

 Method names/signatures and implemented traits are unchanged from `std::io::BufReader`, making replacement as simple as swapping the import of the type:

 ```notest
 - use std::io::BufReader;
 + extern crate buf_redux;
 + use buf_redux::BufReader;
 ```
 ### More Direct Control

 Provides methods to:

 * Access the buffer through an `&`-reference without performing I/O
 * Force unconditional reads into the buffer
 * Shuffle bytes down to the beginning of the buffer to make room for more reading
 * Increase the capacity of the buffer
 * Get the number of available bytes as well as the total capacity of the buffer
 * Consume the `BufReader` without losing data
 * Get inner reader and trimmed buffer with the remaining data
 * Get a `Read` adapter which empties the buffer and then pulls from the inner reader directly 

 ### More Sensible and Customizable Buffering Behavior
 * Tune the behavior of the buffer to your specific use-case using the types in the `strategy`
 module:
     * `BufReader` performs reads as dictated by the `ReadStrategy`
     trait.
     * `BufReader` shuffles bytes down to the beginning of the buffer, to make more room at the end, when deemed appropriate by the
 `MoveStrategy` trait.
     * Feel free to ignore these traits if the default behavior works for you!
 * `BufReader` uses exact allocation instead of leaving it up to `Vec`, which allocates sizes in powers of two.
     * Vec's behavior is more efficient for frequent growth, but much too greedy for infrequent growth and custom capacities.

## Usage

####[Documentation](http://cybergeek94.github.io/buf_redux/buf_redux/)

`Cargo.toml`:
```toml
[dependencies]
buf_redux = "0.2"
```

`lib.rs` or `main.rs`:
```rust
extern crate buf_redux;
```

And then find-and-replace `use std::io::BufReader` with `use buf_redux::BufReader` using whatever tool you prefer.

## License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
