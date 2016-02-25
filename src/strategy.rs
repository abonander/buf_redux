//! Types which can be used to tune the behavior of `BufReader`.
//!
//! Some simple strategies are provided for your convenience. You may prefer to create your own
//! types and implement the traits for them instead.

use std::fmt;

/// Trait for types which `BufReader` can consult to determine when it should read more data into the
/// buffer.
pub trait ReadStrategy: Default + fmt::Debug {
    /// Returns `true` if the buffer should read more data, `false` otherwise.
    ///
    /// Parameters:
    ///
    /// * `start`: The starting index of the valid data region in the buffer (inclusive).
    /// * `end`: The ending index of the valid data region in the buffer (exclusive).
    /// * `buf_size`: The total size of the buffer.
    ///
    /// `end - start` = number of valid bytes in the buffer.
    ///
    /// Implementors may assume `end - start >= 0`
    fn should_read(&self, start: usize, end: usize, buf_size: usize) -> bool;
}

/// A `ReadStrategy` which tells the buffer to read more data only when empty.
///
/// Default behavior of `std::io::BufReader`.
#[derive(Debug, Default)]
pub struct IfEmpty;

impl ReadStrategy for IfEmpty {
    #[inline]
    fn should_read(&self, start: usize, end: usize, _: usize) -> bool {
        end - start == 0
    }
}

/// A `ReadStrategy` which returns `true` if there is fewer bytes in the buffer
/// than the provided value.
#[derive(Debug, Default)]
pub struct LessThan(pub usize);

impl ReadStrategy for LessThan { 
    fn should_read(&self, start: usize, end: usize, _: usize) -> bool { 
        end - start < self.0
    }
}

/// Trait for types which `BufReader` can consult to determine when it should move data
/// to the beginning of the buffer.
///
/// **Note**: If the buffer is empty, the next read will start at the beginning of the buffer
/// regardless of the provided strategy.
pub trait MoveStrategy: Default + fmt::Debug {
    /// Returns `true` if the buffer should move the data down to the beginning, 
    /// `false` otherwise.
    ///
    /// Parameters:
    ///
    /// * `start`: The starting index of the valid data region in the buffer (inclusive).
    /// * `end`: The ending index of the valid data region in the buffer (exclusive).
    /// * `buf_size`: The total size of the buffer.
    ///
    /// `end - start` = number of valid bytes in the buffer.
    ///
    /// Implementors may assume `end - start >= 0`
    fn should_move(&self, start: usize, end: usize, buf_size: usize) -> bool;
}

/// A `MoveStrategy` which tells the buffer to move data if there is no more room at the tail
/// of the buffer, *and* if there is less than **1 KiB** of valid data in the buffer.
///
/// This avoids excessively large copies while still making room for more reads when appropriate.
///
/// Use the `AtEndLessThan` type to set a different threshold.
#[derive(Debug, Default)]
pub struct AtEndLessThan1k;

impl MoveStrategy for AtEndLessThan1k { 
    #[inline]
    fn should_move(&self, start: usize, end: usize, buf_size: usize) -> bool { 
        end == buf_size && end - start < 1024
    }
}

/// A `MoveStrategy` which triggers if there is no more room at the tail at the end of the buffer,
/// *and* there are fewer valid bytes in the buffer than the provided value.
///
/// `AtEndLessThan(1)` is equivalent to `AtEnd`.
/// `AtEndLessThan(1024)` is equivalent to `AtEndLessThan1k`.
#[derive(Debug, Default)]
pub struct AtEndLessThan(pub usize);

impl MoveStrategy for AtEndLessThan { 
    fn should_move(&self, start: usize, end: usize, buf_size: usize) -> bool {
        end == buf_size && end - start < self.0
    }
}

/// A `MoveStrategy` which always returns `false`. Use this to restore original
/// `std::io::BufReader` behavior.
#[derive(Debug, Default)]
pub struct NeverMove;

impl MoveStrategy for NeverMove {
    #[inline]
    fn should_move(&self, _: usize, _: usize, _: usize) -> bool {
        false
    }
}
