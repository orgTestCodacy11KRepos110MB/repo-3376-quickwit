// Copyright (C) 2022 Quickwit, Inc.
//
// Quickwit is offered under the AGPL v3.0 and as commercial software.
// For commercial licensing, contact us at hello@quickwit.io.
//
// AGPL:
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <http://www.gnu.org/licenses/>.

use std::marker::PhantomData;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Poll, Context};

use async_trait::async_trait;
use quickwit_actors::{Mailbox, Actor};
use quickwit_cluster::ClusterMember;
use quickwit_config::service::QuickwitService;
use quickwit_proto::search_service_client::SearchServiceClient;
use quickwit_proto::{SearchRequest, SearchResponse};
use tokio::sync::mpsc::Receiver;
use tokio_stream::Stream;
use tonic::transport::Channel;
use tower::discover::Change;

type DiscoverResult<K, S, E> = Result<Change<K, S>, E>;

pub trait Service: Clone + Send + Sync + 'static {
    /// Returns the [`QuickwitService`] of the client.
    fn qw_service() -> QuickwitService;
    /// Builds a client from a [`SocketAddr`].
    fn build(addr: SocketAddr) -> anyhow::Result<Self>;
}

pub struct DiscoverServiceStream<T: Service> {
    changes: Receiver<Change<SocketAddr, ClusterMember>>,
    _phantom: PhantomData<T>,
}

impl<T: Service> DiscoverServiceStream<T> {
    pub fn new(changes: Receiver<Change<SocketAddr, ClusterMember>>) -> Self {
        Self { changes, _phantom: PhantomData }
    }
}

impl<T: Service> Stream for DiscoverServiceStream<T> {
    type Item = DiscoverResult<SocketAddr, T, anyhow::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let c = &mut self.changes;
        match Pin::new(&mut *c).poll_recv(cx) {
            Poll::Pending | Poll::Ready(None) => Poll::Pending,
            Poll::Ready(Some(change)) => match change {
                Change::Insert(address, cluster_member) => {
                    let new_opt_service = if cluster_member.enabled_services.contains(&T::qw_service()) {
                        let service = T::build(address)
                            .map(|client| Change::Insert(address, client));
                        Some(service)
                    } else {
                        None
                    };
                    Poll::Ready(new_opt_service)
                }
                Change::Remove(k) => Poll::Ready(Some(Ok(Change::Remove(k)))),
            },
        }
    }
}

// Are we sure about that????
impl<T: Service> Unpin for DiscoverServiceStream<T> {}

#[async_trait]
pub trait SearchService {
    async fn root_search(
        &mut self,
        request: impl tonic::IntoRequest<SearchRequest> + Send + Sync,
    ) -> Result<tonic::Response<SearchResponse>, tonic::Status>;
}

#[derive(Clone)]
struct SearchClient {
    inner: SearchServiceClient<Channel>,
}

#[async_trait]
impl SearchService for SearchClient where
{
    async fn root_search(
        &mut self,
        request: impl tonic::IntoRequest<SearchRequest> + Send + Sync,
    ) -> Result<tonic::Response<SearchResponse>, tonic::Status> {
        self.inner.root_search(request).await
    }
}


struct LocalSearchClient<T: Actor> {
    inner: Mailbox<T>
}

#[async_trait]
impl<T: Actor> SearchService for LocalSearchClient<T> {
    async fn root_search(
        &mut self,
        request: impl tonic::IntoRequest<SearchRequest> + Send + Sync,
    ) -> Result<tonic::Response<SearchResponse>, tonic::Status> {
        let response = SearchResponse::default();
        Ok(tonic::Response::new(response))
    }
}
