//! The asset bundler, for the site's integration tests.
//!
//! It lives in its own crate on purpose: Topcoat 0.10's macros refer to `topcoat-asset`
//! directly whenever the calling crate lists it, dev-dependencies included, which breaks
//! normal builds of the site. Depending on this crate instead keeps the site's own
//! dependencies on the `topcoat` facade.

pub use topcoat_asset::{Bundler, BundlerConfig};
