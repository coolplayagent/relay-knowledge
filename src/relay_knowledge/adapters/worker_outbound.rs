//! QoS-governed network adapter for external worker JSON requests.

use crate::{
    net::{NetworkRuntime, http},
    ports::worker_outbound::{WorkerOutboundError, WorkerOutboundFuture, WorkerOutboundPort},
};

#[derive(Clone)]
pub struct NetworkWorkerOutbound {
    network: NetworkRuntime,
}

impl NetworkWorkerOutbound {
    pub fn new(network: NetworkRuntime) -> Self {
        Self { network }
    }
}

impl WorkerOutboundPort for NetworkWorkerOutbound {
    fn post_json<'a>(
        &'a self,
        endpoint: &'a str,
        payload: &'a serde_json::Value,
    ) -> WorkerOutboundFuture<'a> {
        Box::pin(async move {
            let network = self.network.current();
            http::post_json_with_qos(
                &network.http,
                &self.network.qos_runtime(),
                &network.qos,
                endpoint,
                payload,
            )
            .await
            .map_err(|error| WorkerOutboundError {
                message: error.to_string(),
            })
        })
    }
}
