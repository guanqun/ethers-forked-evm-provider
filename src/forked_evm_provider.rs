use crate::akula::evm::execute;
use crate::akula::intra_block_state::IntraBlockState;
use crate::akula::types::PartialHeader;
use crate::forked_backend::Web3RemoteState;
use async_trait::async_trait;
use ethers::abi::ethereum_types::{H256, U256};
use ethers::core::types::transaction::eip2718::TypedTransaction;
use ethers::core::types::{BlockId, NameOrAddress};
use ethers::prelude::{PendingTransaction, Provider, Ws};
use ethers::providers::{JsonRpcClient, Middleware, ProviderError};
use ethers::types::{Bytes, U64};
use evmodin::Revision;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::borrow::BorrowMut;
use std::cell::RefCell;
use std::fmt::Debug;
use std::ops::DerefMut;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug)]
pub struct ForkedEvmProvider {
    backend: Arc<Mutex<IntraBlockState<Web3RemoteState>>>,
}

impl ForkedEvmProvider {
    /// A file path, if that path exists, we don't send request to remote RPC calls.
    /// An URL to access archive node. It would only be used when the above file doesn't exist or log query.
    /// A state snapshot number, it's used to ensure everything matches.
    pub async fn new(block_number: u64, archive_wss_url: &str) -> anyhow::Result<Self> {
        let remote_state = Web3RemoteState::new(block_number, archive_wss_url).await?;
        let intra_block_state = IntraBlockState::new(remote_state);

        Ok(Self {
            backend: Arc::new(Mutex::new(intra_block_state)),
        })
    }
}

#[async_trait]
impl JsonRpcClient for ForkedEvmProvider {
    type Error = ProviderError;

    async fn request<T, R>(&self, _method: &str, _params: T) -> Result<R, Self::Error>
    where
        T: Debug + Serialize + Send + Sync,
        R: Serialize + DeserializeOwned,
    {
        unreachable!("It shall not send out actual requests.")
    }
}

#[async_trait]
impl Middleware for ForkedEvmProvider {
    type Error = ProviderError;
    type Provider = Self;
    type Inner = Self;

    fn inner(&self) -> &Self::Inner {
        unreachable!("There is no inner provider here")
    }

    async fn get_block_number(&self) -> Result<U64, Self::Error> {
        Ok(1234.into())
    }

    async fn call(
        &self,
        tx: &TypedTransaction,
        block: Option<BlockId>,
    ) -> Result<Bytes, Self::Error> {
        let header = PartialHeader::default();

        let mut lock = self.backend.lock().await;

        let ret = execute(lock.deref_mut(), &header, Revision::London, tx, i64::MAX)
            .await
            .unwrap();

        Ok(ret.output_data.into())
    }

    async fn send_raw_transaction(
        &self,
        tx: Bytes,
    ) -> Result<PendingTransaction<Self::Provider>, Self::Error> {
        todo!()
    }

    async fn get_storage_at<T: Into<NameOrAddress> + Send + Sync>(
        &self,
        address: T,
        location: H256,
        block: Option<BlockId>,
    ) -> Result<H256, Self::Error> {
        let address = match address.into() {
            NameOrAddress::Name(_) => {
                todo!()
            }
            NameOrAddress::Address(address) => address,
        };

        let mut lock = self.backend.lock().await;
        Ok(lock
            .get_current_storage(address, location)
            .await
            .map_err(|e| ProviderError::CustomError(format!("{:?}", e)))?)
    }
}
