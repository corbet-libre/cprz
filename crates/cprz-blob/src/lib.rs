//! Lossless byte compression using maintained upstream codecs.
//!
//! [`portable`] provides bounded Brotli decoding on native and wasm targets.
//! The default `zstd` feature preserves the original zstd API.

/// Portable exact-byte compression with explicit expansion limits.
pub mod portable;

#[cfg(feature = "zstd")]
mod legacy;
#[cfg(feature = "zstd")]
pub use legacy::*;
