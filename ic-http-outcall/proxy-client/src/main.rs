use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use candid::Principal;
use clap::Parser;
use futures::future;
use ic_canister_client::agent::identity;
use ic_canister_client::{CanisterClient, IcAgentClient};
use ic_exports::ic_cdk::api::management_canister::http_request::{
    CanisterHttpRequestArgument, HttpHeader as IcHttpHeader, HttpMethod as IcHttpMethod,
    HttpResponse,
};
use ic_http_outcall_api::{RequestId, ResponseResult};
use reqwest::header::{HeaderName, HeaderValue};
use tokio::time::Instant;

#[derive(Debug, Parser)]
pub struct ProxyClientArgs {
    /// Path to your identity pem file
    #[arg(short = 'i', long = "identity")]
    pub identity: PathBuf,

    /// IC Network url
    /// Use https://icp0.io for the Internet Computer Mainnet.
    #[arg(short, long, default_value = "http://127.0.0.1:8000")]
    pub network_url: String,

    /// Proxy canister principal
    #[arg(short, long = "canister")]
    pub canister: Principal,

    /// Timeout for requestst to proxy canister in millis.
    #[arg(long, default_value = "5000")]
    pub timeout: u64,

    /// Proxy canister query period in millis.
    #[arg(long, default_value = "200")]
    pub query_period: u64,

    /// Max Http requests batch size.
    #[arg(long, default_value = "20")]
    pub batch_size: usize,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let args = ProxyClientArgs::parse();
    log::info!("Starting with args: {args:?}.");

    let agent = identity::init_agent(
        args.identity,
        &args.network_url,
        Some(Duration::from_millis(args.timeout)),
    )
    .await?;
    let agent_client = IcAgentClient::with_agent(args.canister, agent);

    log::info!("Agent client initialized.");

    let query_period = Duration::from_millis(args.query_period);
    let client = ProxyClient::new(agent_client, query_period, args.batch_size);

    log::info!("Running requests processing");
    client.run().await;

    Ok(())
}

struct ProxyClient {
    client: IcAgentClient,
    query_period: Duration,
    batch_size: usize,
}

impl fmt::Debug for ProxyClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyClient")
            .field("client", &self.client.canister_id)
            .finish()
    }
}

impl ProxyClient {
    pub fn new(client: IcAgentClient, query_period: Duration, batch_size: usize) -> Self {
        Self {
            client,
            query_period,
            batch_size,
        }
    }

    pub async fn run(self) {
        // In case of outdated results of `get_pending_requests` query, store
        // finished requests ids on last iteration.
        let mut just_finished_ids = HashSet::with_capacity(self.batch_size);
        let mut start_next_itration_at = Instant::now();

        loop {
            let now = Instant::now();
            if now < start_next_itration_at {
                let wait_for = start_next_itration_at - now;
                log::trace!("Waiting for {} millis", wait_for.as_millis());
                tokio::time::sleep(wait_for).await;
            }

            start_next_itration_at = Instant::now() + self.query_period;

            log::info!("Query for Http requests");

            let Ok(mut requests) = self
                .get_pending_requests()
                .await
                .inspect_err(|e| log::warn!("Failed to get pending requests: {e}."))
            else {
                continue;
            };

            requests.retain(|r| !just_finished_ids.contains(&r.0));

            if requests.is_empty() {
                continue;
            }

            log::info!("Processing {} requests", requests.len());

            let responses = self.perform_requests(requests).await;
            if let Err(e) = self.finish_requests(&responses).await {
                log::warn!("Failed to finish responses: {e}.");
            };

            just_finished_ids = responses.into_iter().map(|r| r.id).collect();
        }
    }

    async fn get_pending_requests(
        &self,
    ) -> anyhow::Result<Vec<(RequestId, CanisterHttpRequestArgument)>> {
        Ok(self
            .client
            .query("pending_requests", (self.batch_size,))
            .await?)
    }

    async fn perform_requests(
        &self,
        requests: Vec<(RequestId, CanisterHttpRequestArgument)>,
    ) -> Vec<ResponseResult> {
        let response_futures = requests.into_iter().map(|(id, args)| async move {
            let response = Self::perform_request(args).await;
            ResponseResult {
                id,
                result: response.map_err(|e| e.to_string()),
            }
        });

        future::join_all(response_futures).await
    }

    async fn perform_request(args: CanisterHttpRequestArgument) -> anyhow::Result<HttpResponse> {
        let method = match args.method {
            IcHttpMethod::GET => reqwest::Method::GET,
            IcHttpMethod::POST => reqwest::Method::POST,
            IcHttpMethod::HEAD => reqwest::Method::HEAD,
        };
        let url = match reqwest::Url::parse(&args.url) {
            Ok(url) => url,
            Err(e) => {
                anyhow::bail!("Failed to parse URL from '{}': {e}.", args.url)
            }
        };

        let headers = args.headers.into_iter().filter_map(|h| {
            let header_name = HeaderName::from_str(&h.name)
                .inspect_err(|e| {
                    log::warn!(
                        "Failed to parse HeaderName from '{}': {e}. The header is skipped.",
                        h.value
                    )
                })
                .ok()?;
            let header_value = HeaderValue::from_str(&h.value)
                .inspect_err(|e| {
                    log::warn!(
                        "Failed to parse HeaderValue from '{}': {e}. The header is skipped.",
                        h.value
                    )
                })
                .ok()?;

            Some((header_name, header_value))
        });

        let response = reqwest::Client::new()
            .request(method, url)
            .headers(headers.collect())
            .body(args.body.unwrap_or_default())
            .send()
            .await?;

        Self::into_ic_response(response).await
    }

    async fn into_ic_response(reqwest_response: reqwest::Response) -> anyhow::Result<HttpResponse> {
        let status = reqwest_response.status().as_u16().into();

        let headers = reqwest_response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                let value = value
                    .to_str()
                    .inspect_err(|e| {
                        log::warn!(
                        "Failed to convert response header to string: {e}. Header will be skipped."
                    )
                    })
                    .ok()?
                    .into();

                Some(IcHttpHeader {
                    name: name.to_string(),
                    value,
                })
            })
            .collect();

        let body = reqwest_response.bytes().await?;
        Ok(HttpResponse {
            status,
            headers,
            body: body.into(),
        })
    }

    async fn finish_requests(&self, responses: &[ResponseResult]) -> anyhow::Result<()> {
        Ok(self.client.update("finish_requests", (&responses,)).await?)
    }
}
