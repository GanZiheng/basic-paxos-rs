#![allow(dead_code)]

#[cfg(test)]
mod tests;

use pb::Proposal;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Mutex;
use tonic::transport::Server;

pub mod pb {
    tonic::include_proto!("paxos");
}

#[derive(Default)]
struct AcceptorInner {
    promise_number: i64,
    highest_numbered_proposal: Option<Proposal>,
}

#[derive(Default)]
pub struct Acceptor {
    inner: Mutex<AcceptorInner>,
}

#[tonic::async_trait]
impl pb::paxos_server::Paxos for Acceptor {
    async fn prepare(
        &self,
        request: tonic::Request<pb::PrepareRequest>,
    ) -> Result<tonic::Response<pb::PrepareResponse>, tonic::Status> {
        let mut inner = self.inner.lock().await;

        let number = request.into_inner().number;

        inner.promise_number = std::cmp::max(number, inner.promise_number);

        let response = pb::PrepareResponse {
            promise_number: inner.promise_number,
            highest_numbered_proposal: inner.highest_numbered_proposal.clone(),
        };

        Ok(tonic::Response::new(response))
    }

    async fn accept(
        &self,
        request: tonic::Request<pb::AcceptRequest>,
    ) -> Result<tonic::Response<pb::AcceptResponse>, tonic::Status> {
        let mut inner = self.inner.lock().await;

        let proposal = request.into_inner().proposal.unwrap();
        assert!(proposal.number <= inner.promise_number);

        if proposal.number == inner.promise_number {
            inner.highest_numbered_proposal = Some(proposal);
        }

        let response = pb::AcceptResponse {
            promise_number: inner.promise_number,
        };

        Ok(tonic::Response::new(response))
    }
}

pub struct Paxos {
    acceptor_num: u32,
    acceptor_addresses: Vec<String>,
    runtime: Runtime,
}

impl Paxos {
    pub fn new(acceptor_num: u32) -> Self {
        let runtime = Builder::new_multi_thread()
            .thread_name("paxos")
            .worker_threads(4)
            .enable_io()
            .build()
            .unwrap();

        let acceptor_addresses = (0..acceptor_num)
            .map(|i| {
                let addr_str = format!("[::1]:5005{}", i);
                let addr = addr_str.parse().unwrap();
                let acceptor = Acceptor::default();
                let svc = pb::paxos_server::PaxosServer::new(acceptor);

                runtime.spawn(async move {
                    Server::builder()
                        .add_service(svc)
                        .serve(addr)
                        .await
                        .unwrap();
                });

                addr_str
            })
            .collect();

        Self {
            acceptor_num,
            acceptor_addresses,
            runtime,
        }
    }
}
