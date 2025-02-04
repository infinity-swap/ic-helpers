//! Abstraction over Http outcalls.

use ic_exports::ic_cdk::api::management_canister::http_request::{
    CanisterHttpRequestArgument, HttpResponse,
};
use ic_exports::ic_kit::CallResult;

/// Abstraction over Http outcalls.
#[allow(async_fn_in_trait)]
pub trait HttpOutcall {
    async fn request(&self, args: CanisterHttpRequestArgument) -> CallResult<HttpResponse>;
}
