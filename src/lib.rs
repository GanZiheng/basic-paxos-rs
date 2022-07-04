#![allow(dead_code)]

#[cfg(test)]
mod tests;

use futures::future::join_all;
use pb::{AcceptResponse, PrepareResponse, Proposal};
use rand::Rng;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tonic::transport::{Channel, Server};
use tonic::Status;

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

        inner.promise_number = std::cmp::max(inner.promise_number, number);

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
    runtime: Option<Runtime>,
    current_number: Mutex<i64>,
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
                let addr_str = format!("[::1]:5{:04}", i);
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
            runtime: Some(runtime),
            current_number: Mutex::new(0),
        }
    }

    pub async fn run_paxos(&self, value: String) -> Proposal {
        let quorum = self.acceptor_num / 2 + 1;
        let mut rng = rand::thread_rng();

        loop {
            let number = {
                let mut current_number = self.current_number.lock().await;
                *current_number += 1;
                *current_number
            };

            let responses = self.phase1(number).await;

            let mut outdate = false;
            let mut address_indexes = Vec::new();
            let mut highest_number = 0;
            let mut highest_numbered_proposal = None;

            responses
                .iter()
                .enumerate()
                .for_each(|(index, response)| match response {
                    Ok(response) => {
                        if response.promise_number > number {
                            outdate = true;
                            return;
                        }
                        assert!(response.promise_number == number);

                        address_indexes.push(index);

                        if response.highest_numbered_proposal.is_none() {
                            return;
                        }

                        let proposal = response.highest_numbered_proposal.clone().unwrap();
                        if proposal.number > highest_number {
                            highest_number = proposal.number;
                            highest_numbered_proposal = Some(proposal);
                        }
                    }
                    Err(_) => (),
                });

            if outdate {
                sleep(Duration::from_millis(rng.gen_range(3000..5000))).await;
                continue;
            }

            if address_indexes.len() < quorum.try_into().unwrap() {
                sleep(Duration::from_secs(5)).await;
                continue;
            }

            let mut proposal = Proposal {
                number,
                value: Default::default(),
            };
            proposal.value = if highest_numbered_proposal.is_none() {
                value.clone()
            } else {
                highest_numbered_proposal.unwrap().value
            };

            let responses = self.phase2(proposal.clone(), address_indexes).await;

            responses.iter().for_each(|response| match response {
                Ok(response) => {
                    if response.promise_number > number {
                        outdate = true;
                        return;
                    }
                    assert!(response.promise_number == number);
                }
                Err(_) => (),
            });

            if outdate {
                sleep(Duration::from_millis(rng.gen_range(3000..5000))).await;
                continue;
            }

            return proposal;
        }
    }

    async fn phase1(&self, number: i64) -> Vec<Result<PrepareResponse, Status>> {
        join_all(self.acceptor_addresses.iter().map(|addr| async move {
            let channel = Channel::from_shared(format!("http://{}", addr))
                .unwrap()
                .timeout(Duration::from_secs(5))
                .connect()
                .await
                .unwrap();

            let mut client = pb::paxos_client::PaxosClient::new(channel);

            let request = pb::PrepareRequest { number };

            match client.prepare(request).await {
                Ok(response) => {
                    println!("response: {:?}", response);
                    Ok(response.into_inner())
                }
                Err(e) => Err(e),
            }
        }))
        .await
    }

    async fn phase2(
        &self,
        proposal: Proposal,
        address_indexes: Vec<usize>,
    ) -> Vec<Result<AcceptResponse, Status>> {
        join_all(address_indexes.into_iter().map(|addr| {
            let proposal = proposal.clone();
            let addr = self.acceptor_addresses[addr].clone();
            async move {
                let channel = Channel::from_shared(format!("http://{}", addr))
                    .unwrap()
                    .timeout(Duration::from_secs(5))
                    .connect()
                    .await
                    .unwrap();

                let mut client = pb::paxos_client::PaxosClient::new(channel);

                let request = pb::AcceptRequest {
                    proposal: Some(proposal),
                };

                match client.accept(request).await {
                    Ok(response) => {
                        println!("response: {:?}", response);
                        Ok(response.into_inner())
                    }
                    Err(e) => Err(e),
                }
            }
        }))
        .await
    }
}

impl Drop for Paxos {
    fn drop(&mut self) {
        self.runtime.take().unwrap().shutdown_background();
    }
}
