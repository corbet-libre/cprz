//! Compression facade for storage and transport payloads.
//!
//! Use [`portable`] with `default-features = false` for bounded, exact-byte
//! native and wasm compression. Apply Padme padding and encryption afterwards.
//! The default `native` feature preserves the existing text/image/delta facade.

pub use cprz_blob::portable;

#[cfg(feature = "native")]
mod legacy;
#[cfg(feature = "native")]
pub use legacy::*;
