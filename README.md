# buf\_re(a)dux

A more useful implementation of Rust's `std::io::BufReader`. 

Features include:

* More direct control over the buffer. Provides methods to:
  * Access the buffer through an `&`-reference without performing I/O
  * Force unconditional reads into the buffer
  * Increase the capacity of the buffer
  * Get the number of available bytes as well as the total capacity of the buffer
  * Consume the `BufReader` without losing data
    * Get inner reader and trimmed buffer with the remaining data
    * Get a `Read` adapter which empties the buffer and then pulls from the inner reader directly
* More sensible buffering behavior
  * Data is moved down to the beginning of the buffer when appropriate
    * Such as when there is more room at the beginning of the buffer than at the end
  * Exact allocation instead of leaving it up to `Vec`, which allocates sizes in powers of two
    * Vec's behavior is more efficient for frequent growth, but much too greedy for infrequent growth and custom capacities.
* Drop-in replacement
  * Method names/signatures and implemented traits are unchanged from `std::io::BufReader`, making replacement stupidly simple.

## License

Licensed under either of

 * Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.
