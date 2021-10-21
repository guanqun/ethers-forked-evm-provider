use crate::akula::interface::State;
use crate::akula::types::{Account, Incarnation, PartialHeader};
use crate::akula::utils::keccak256;
use async_trait::async_trait;
use bytes::Bytes;
use ethers::prelude::*;
use futures::future;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Web3RemoteState {
    provider: Provider<Ws>,
    block_number: u64,
    code_hash_map: Arc<Mutex<HashMap<H256, Bytes>>>,
}

impl Web3RemoteState {
    pub async fn new(block_number: u64, ws_url: &str) -> anyhow::Result<Self> {
        let ws = Ws::connect(ws_url).await?;
        let provider = Provider::new(ws);

        Ok(Self {
            provider,
            block_number,
            code_hash_map: Arc::new(Mutex::new(Default::default())),
        })
    }
}

impl Debug for Web3RemoteState {
    fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

#[async_trait]
impl State for Web3RemoteState {
    async fn read_account(&self, address: Address) -> anyhow::Result<Option<Account>> {
        let (balance, nonce, code) = future::try_join3(
            self.provider
                .get_balance(address, Some(self.block_number.into())),
            self.provider
                .get_transaction_count(address, Some(self.block_number.into())),
            self.provider
                .get_code(address, Some(self.block_number.into())),
        )
        .await?;

        if balance.is_zero() && nonce.is_zero() && code.0.is_empty() {
            Ok(None)
        } else {
            // cache the code has, it would be used later on by read_code()
            let code_hash = keccak256(code.0.as_ref());
            {
                let mut lock = self.code_hash_map.lock().await;
                lock.insert(code_hash, code.0.clone());
            }

            Ok(Some(Account {
                nonce: nonce.as_u64(),
                balance,
                code_hash,
                incarnation: Default::default(),
            }))
        }
    }

    async fn read_code(&self, code_hash: H256) -> anyhow::Result<Bytes> {
        let lock = self.code_hash_map.lock().await;
        Ok(lock.get(&code_hash).cloned().unwrap())
    }

    async fn read_storage(
        &self,
        address: Address,
        _incarnation: Incarnation,
        location: H256,
    ) -> anyhow::Result<H256> {
        Ok(self
            .provider
            .get_storage_at(address, location, Some(self.block_number.into()))
            .await?)
    }

    async fn read_block_header(&self, block_number: u64) -> anyhow::Result<Option<PartialHeader>> {
        let block = self.provider.get_block(block_number).await?;
        Ok(block.map(|b| PartialHeader {
            parent_hash: b.parent_hash,
            beneficiary: b.author,
            state_root: Default::default(),
            receipts_root: Default::default(),
            difficulty: b.difficulty,
            number: self.block_number,
            gas_limit: b.gas_limit.as_u64(),
            gas_used: b.gas_used.as_u64(),
            timestamp: b.timestamp.as_u64(),
            extra_data: b.extra_data.0.clone(),
            mix_hash: b.mix_hash.unwrap_or_default(),
            nonce: Default::default(),
            base_fee_per_gas: b.base_fee_per_gas,
        }))
    }

    /// This is used for blockhash opcode.
    async fn read_block_hash(&self, block_number: u64) -> anyhow::Result<H256> {
        let block = self.provider.get_block(block_number).await?;
        Ok(block.map(|b| b.hash).flatten().unwrap_or_default())
    }
}
