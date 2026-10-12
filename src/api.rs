//! Protocol adapters. Pure contracts are available without native storage.
pub mod aml;
mod aml_protocol;
#[cfg(feature = "local-storage")]
mod http;
#[cfg(feature = "local-storage")]
pub use http::{ApiOptions, router, router_with_options};
