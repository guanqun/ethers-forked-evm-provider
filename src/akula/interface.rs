use crate::akula::types::{Account, Incarnation, PartialHeader};
use async_trait::async_trait;
use bytes::Bytes;
use ethers::types::{Address, H256};
use std::fmt::Debug;

#[async_trait]
pub trait State: Debug + Send + Sync {
    async fn read_account(&self, address: Address) -> anyhow::Result<Option<Account>>;

    async fn read_code(&self, code_hash: H256) -> anyhow::Result<Bytes>;

    async fn read_storage(
        &self,
        address: Address,
        incarnation: Incarnation,
        location: H256,
    ) -> anyhow::Result<H256>;

    // Previous non-zero incarnation of an account; 0 if none exists.
    async fn previous_incarnation(&self, _address: Address) -> anyhow::Result<Incarnation> {
        Ok(Incarnation(0))
    }

    async fn read_block_header(&self, block_number: u64) -> anyhow::Result<Option<PartialHeader>>;
}
