use crate::akula::types::{Account, Incarnation, PartialHeader};
use async_trait::async_trait;
use auto_impl::auto_impl;
use bytes::Bytes;
use ethers::types::{Address, H256, U256};
use std::fmt::Debug;

#[async_trait]
// #[auto_impl(&mut, Box)]
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
    async fn previous_incarnation(&self, address: Address) -> anyhow::Result<Incarnation>;

    async fn read_header(
        &self,
        block_number: u64,
        block_hash: H256,
    ) -> anyhow::Result<Option<PartialHeader>>;

    // async fn read_body(
    //     &self,
    //     block_number: u64,
    //     block_hash: H256,
    // ) -> anyhow::Result<Option<BlockBody>>;

    // async fn read_body_with_senders(
    //     &self,
    //     block_number: u64,
    //     block_hash: H256,
    // ) -> anyhow::Result<Option<BlockBodyWithSenders>>;

    async fn total_difficulty(
        &self,
        block_number: u64,
        block_hash: H256,
    ) -> anyhow::Result<Option<U256>>;

    // async fn insert_receipts(
    //     &mut self,
    //     block_number: u64,
    //     receipts: Vec<Receipt>,
    // ) -> anyhow::Result<()>;
}
