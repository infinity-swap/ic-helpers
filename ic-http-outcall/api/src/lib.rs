//! This crate provides an abstraction over different implementations of IC
//! Http outcall mechanism.
//!
//! The absraction is described as `HttpOutcall` trait. It has the
//! following implementations:
//! - `NonReplicatedHttpOutcall` - performs non-replicated call.
//!   Details: `https://forum.dfinity.org/t/non-replicated-https-outcalls/26627`;
//!
//! - `ReplicatedHttpOutcall` - perform replicated calls using basic `http_outcall`
//!   method in IC API.

#[cfg(feature = "non-replicated")]
mod non_replicated;
#[cfg(feature = "proxy-api")]
mod proxy_types;

mod outcall;
mod replicated;

#[cfg(feature = "non-replicated")]
pub use non_replicated::NonReplicatedHttpOutcall;
pub use outcall::HttpOutcall;
#[cfg(feature = "proxy-api")]
pub use proxy_types::{InitArgs, RequestArgs, RequestId, ResponseResult, REQUEST_METHOD_NAME};
pub use replicated::ReplicatedHttpOutcall;
