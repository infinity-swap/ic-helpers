use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use candid::Principal;
use futures::channel::oneshot;
use ic_canister::virtual_canister_call;
use ic_exports::ic_cdk::api::management_canister::http_request::{
    CanisterHttpRequestArgument, HttpResponse,
};
use ic_exports::ic_kit::{ic, CallResult, RejectionCode};

use crate::outcall::HttpOutcall;
use crate::proxy_types::{RequestArgs, RequestId, REQUEST_METHOD_NAME};
use crate::ResponseResult;

/// Callback type, which should be called to make the `HttpOutcall::request` function return.
pub type OnResponse = Box<dyn Fn(Vec<ResponseResult>)>;

/// Non-replicated http outcall implementation, which works together with ProxyCanister and ProxyCanisterClient.
///
/// # Workflow
/// 1. Client code calls `HttpOutcall::request(params)`. `Self` sends request params to ProxyCanister
///    and awaits a Waker, which will be awaken, once the ProxyCanister will call the given callback,
///    or when the timeout reached.
///
/// 2. ProxyCanister stores request params, and waits until ProxyCanisterClient query and execute the request.
///
/// 3. ProxyCanister notifies the current canister about the reponse, by calling the `callback_api_fn_name` API endpoint.
///    The notification should be forwarded to the `OnResponse` callback, returned from `Self::new(...)`.
#[derive(Debug)]
pub struct NonReplicatedHttpOutcall {
    requests: Rc<RefCell<HashMap<RequestId, DeferredResponse>>>,
    callback_api_fn_name: &'static str,
    proxy_canister: Principal,
    request_timeout: Duration,
}

impl NonReplicatedHttpOutcall {
    /// Crates a new instance of NonReplicatedHttpOutcall and a callback, which should be called
    /// from canister API `callback_api_fn_name` function for response processing.
    ///
    /// The `callback_api_fn_name` expected to
    /// - have a name equal to `callback_api_fn_name` value,
    /// - be a canister API update endpoint,
    /// - have the following signature: `fn(Vec<ResponseResult>) -> ()`
    /// - call the returned `OnResponse` callback.
    pub fn new(
        proxy_canister: Principal,
        callback_api_fn_name: &'static str,
        request_timeout: Duration,
    ) -> (Self, OnResponse) {
        let s = Self {
            requests: Default::default(),
            callback_api_fn_name,
            proxy_canister,
            request_timeout,
        };

        let requests = Rc::clone(&s.requests);
        let callback = Box::new(move |responses: Vec<ResponseResult>| {
            for response in responses {
                if let Some(deferred) = requests.borrow_mut().remove(&response.id) {
                    let _ = deferred.notify.send(response.result);
                }
            }
        });

        (s, callback)
    }

    /// Checks if some requests are expired, and, if so, finishs them with timeout error.
    pub fn check_requests_timeout(&self) {
        let now = ic::time();
        self.requests
            .borrow_mut()
            .retain(|_, deffered| deffered.expired_at > now);
    }
}

impl HttpOutcall for NonReplicatedHttpOutcall {
    async fn request(&self, request: CanisterHttpRequestArgument) -> CallResult<HttpResponse> {
        self.check_requests_timeout();

        let proxy_canister = self.proxy_canister;
        let request = RequestArgs {
            callback_name: self.callback_api_fn_name.into(),
            request,
        };
        let id: RequestId =
            virtual_canister_call!(proxy_canister, REQUEST_METHOD_NAME, (request,), RequestId)
                .await?;

        let (notify, waker) = oneshot::channel();
        let expired_at = ic::time() + self.request_timeout.as_nanos() as u64;
        let response = DeferredResponse { expired_at, notify };
        self.requests.borrow_mut().insert(id, response);

        waker
            .await
            .map_err(|_| {
                // if proxy canister doesn't respond
                (
                    RejectionCode::SysTransient,
                    "timeout waiting HTTP request callback.".into(),
                )
            })?
            .map_err(|e| {
                // if request failed
                (
                    RejectionCode::SysFatal,
                    format!("failed to send HTTP request: {e}"),
                )
            })
    }
}

#[derive(Debug)]
struct DeferredResponse {
    pub expired_at: u64,
    pub notify: oneshot::Sender<Result<HttpResponse, String>>,
}
