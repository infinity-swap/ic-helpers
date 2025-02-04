use candid::Principal;
use ic_exports::ic_cdk::api::management_canister::http_request::{
    CanisterHttpRequestArgument, HttpResponse,
};
use ic_exports::ic_kit::CallResult;
use ic_helpers::principal::management::ManagementPrincipalExt;

use crate::outcall::HttpOutcall;

pub struct ReplicatedHttpOutcall;

impl HttpOutcall for ReplicatedHttpOutcall {
    async fn request(&self, args: CanisterHttpRequestArgument) -> CallResult<HttpResponse> {
        Principal::management_canister().http_request(args).await
    }
}
