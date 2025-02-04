use candid::Principal;
use ic_exports::candid::CandidType;
use ic_exports::ic_cdk::api::management_canister::http_request::{
    CanisterHttpRequestArgument, HttpResponse,
};
use serde::{Deserialize, Serialize};

pub const REQUEST_METHOD_NAME: &str = "http_outcall";

/// Params for proxy canister initialization.
#[derive(Debug, Serialize, Deserialize, CandidType)]
pub struct InitArgs {
    /// Off-chain proxy agent, allowed to perform http requests.
    pub allowed_proxy: Principal,
}

/// ID of a request.
#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, CandidType, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub struct RequestId(u64);

impl From<u64> for RequestId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Deserialize, CandidType)]
pub struct RequestArgs {
    pub callback_name: String,
    pub request: CanisterHttpRequestArgument,
}

#[derive(Debug, Clone, Serialize, Deserialize, CandidType)]
pub struct ResponseResult {
    pub id: RequestId,
    pub result: Result<HttpResponse, String>,
}
