//! Types which can be used to tune the behavior of `BufReader`.
//!
//! Some simple strategies are provided for your convenience. You may prefer to create your own
//! types and implement the traits for them instead.

use super::Buffer;

use std::fmt;

/// The default `ReadStrategy` for this crate.
pub type DefaultReadStrategy = IfEmpty;
/// The default `MoveStrategy` for this crate.
pub type DefaultMoveStrategy = AtEndLessThan1k;
/// The default `FlushStrategy` for this crate.
pub type DefaultFlushStrategy = WhenFull;

/// Trait for types which `BufReader` can consult to determine when it should read more data into the
/// buffer.
pub trait ReadStrategy: Default + fmt::Debug {
    /// Returns `true` if the buffer should read more data, `false` otherwise.
    fn should_read(&self, buffer: &Buffer) -> bool;
}

/// A `ReadStrategy` which tells the buffer to read more data only when empty.
///
/// Default behavior of `std::io::BufReader`.
#[derive(Debug, Default)]
pub struct IfEmpty;

impl ReadStrategy for IfEmpty {
    #[inline]
    fn should_read(&self, buffer: &Buffer) -> bool {
        buffer.available() == 0
    }
}

/// A `ReadStrategy` which returns `true` if there is fewer bytes in the buffer
/// than the provided value.
#[derive(Debug, Default)]
pub struct LessThan(pub usize);

impl ReadStrategy for LessThan { 
    fn should_read(&self, buffer: &Buffer) -> bool { 
        buffer.available() < self.0
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
    fn should_move(&self, buffer: &Buffer) -> bool;
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
    fn should_move(&self, buffer: &Buffer) -> bool { 
        buffer.headroom() == 0 && buffer.available() < 1024
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
    fn should_move(&self, buffer: &Buffer) -> bool {
        buffer.headroom() == 0 && buffer.available() < self.0
    }
}

/// A `MoveStrategy` which always returns `false`. Use this to restore original
/// `std::io::BufReader` behavior.
#[derive(Debug, Default)]
pub struct NeverMove;

impl MoveStrategy for NeverMove {
    #[inline]
    fn should_move(&self, _: &Buffer) -> bool {
        false
    }
}

/// A trait which tells `BufWriter` when to flush.
pub trait FlushStrategy: Default + fmt::Debug {
    /// Return `true` if the buffer should be flused.
    fn should_flush(&self, buf: &Buffer, incoming: usize) -> bool;
}

/// Flush the buffer if there is no more headroom. Equivalent to the behavior or
/// `std::io::BufWriter`.
#[derive(Debug, Default)]
pub struct WhenFull;

impl FlushStrategy for WhenFull {
    fn should_flush(&self, buf: &Buffer, incoming: usize) -> bool {
        buf.headroom() < incoming
    }
}

/// Flush the buffer if it contains at least the given number of bytes.
#[derive(Debug, Default)]
pub struct FlushAtLeast(pub usize);

impl FlushStrategy for FlushAtLeast {
    fn should_flush(&self, buf: &Buffer, _: usize) -> bool {
        buf.available() > self.0
    }
}

/// Flush the buffer if it contains at least `8Kb (8192b)`.
#[derive(Debug, Default)]
pub struct FlushAtLeast8k;

impl FlushStrategy for FlushAtLeast8k {
    fn should_flush(&self, buf: &Buffer, _: usize) -> bool {
        buf.available() > 8192
    }
}
