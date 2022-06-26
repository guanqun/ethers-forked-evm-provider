use crate::akula::evm::execute;
use crate::akula::interface::State;
use crate::akula::intra_block_state::IntraBlockState;
use crate::akula::types::PartialHeader;
use crate::state_muxer::{BackendConfig, StateMuxer};
use anyhow::anyhow;
use async_trait::async_trait;
use ethers::abi::ethereum_types::H256;
use ethers::core::types::transaction::eip2718::TypedTransaction;
use ethers::core::types::{BlockId, NameOrAddress};
use ethers::providers::{JsonRpcClient, Middleware, PendingTransaction, Provider, ProviderError};
use ethers::types::{Address, Bytes, U64};
use evmodin::{Revision, StatusCode};
use primitive_types::U256;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::convert::TryFrom;
use std::fmt::Debug;
use std::ops::DerefMut;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug)]
pub struct ForkedEvmProvider {
    header: PartialHeader,
    state_block_number: u64,
    backend: Arc<Mutex<IntraBlockState<StateMuxer>>>,

    dummy_provider: Provider<LoopbackProvider>,
}

impl ForkedEvmProvider {
    /// A file path, if that path exists, we don't send request to remote RPC calls.
    /// An URL to access archive node. It would only be used when the above file doesn't exist or log query.
    /// A state snapshot number, it's used to ensure everything matches.
    pub async fn new(
        state_block_number: u64,
        archive_wss_url: &str,
        db_path: PathBuf,
    ) -> anyhow::Result<Self> {
        let config = if db_path.as_path().exists() {
            // use db as the first choice, otherwise use the tee mode
            BackendConfig::LocalOnly { db_path }
        } else {
            BackendConfig::TeeWeb3ToLocal {
                wss_url: archive_wss_url.to_string(),
                db_path,
            }
        };

        let state_mux = StateMuxer::new(state_block_number, config).await?;
        let header = state_mux
            .read_block_header(state_block_number + 1)
            .await?
            .expect("failed to get header");

        let intra_block_state = IntraBlockState::new(state_mux);

        Ok(Self {
            header,
            state_block_number,
            backend: Arc::new(Mutex::new(intra_block_state)),
            dummy_provider: Provider::new(LoopbackProvider),
        })
    }

    pub async fn new_with_remote(
        state_block_number: u64,
        archive_wss_url: &str,
    ) -> anyhow::Result<Self> {
        let state_mux = StateMuxer::new(
            state_block_number,
            BackendConfig::AllViaWeb3 {
                wss_url: archive_wss_url.to_string(),
            },
        )
        .await?;
        let mut header = state_mux
            .read_block_header(state_block_number + 1)
            .await?
            .expect("failed to get header");
        header.number += 1;

        let intra_block_state = IntraBlockState::new(state_mux);

        Ok(Self {
            header,
            state_block_number,
            backend: Arc::new(Mutex::new(intra_block_state)),
            dummy_provider: Provider::new(LoopbackProvider),
        })
    }

    pub async fn deploy(&self, tx: &TypedTransaction) -> anyhow::Result<Address> {
        let mut lock = self.backend.lock().await;
        let ret = execute(
            lock.deref_mut(),
            &self.header,
            Revision::London,
            tx,
            tx.gas().cloned().unwrap_or_default().as_u64() as i64,
        )
        .await
        .unwrap();
        Ok(ret
            .create_address
            .ok_or_else(|| anyhow!("failed to create address"))?)
    }

    pub async fn transact(&self, tx: &TypedTransaction) -> Result<(u64, Vec<u8>), ProviderError> {
        let mut lock = self.backend.lock().await;
        let ret = execute(
            lock.deref_mut(),
            &self.header,
            Revision::London,
            tx,
            i64::MAX,
        )
        .await
        .unwrap();

        // only return the output data if it's successful
        if ret.status_code == StatusCode::Success {
            Ok(((i64::MAX - ret.gas_left) as u64, ret.output_data.to_vec()))
        } else {
            Err(ProviderError::CustomError(format!(
                "reverted with {:?}",
                ret.output_data
            )))
        }
    }
}

#[derive(Debug)]
pub struct LoopbackProvider;

#[async_trait]
impl JsonRpcClient for LoopbackProvider {
    type Error = ProviderError;

    /// This is used by PendingTransaction only.
    ///         self.request("eth_getTransactionByHash", [hash]).await
    /// we can safely panic on other cases.
    async fn request<T, R>(&self, _method: &str, _params: T) -> Result<R, Self::Error>
    where
        T: Debug + Serialize + Send + Sync,
        R: DeserializeOwned,
    {
        unreachable!("It shall not send out actual requests.")
    }
}

#[async_trait]
impl Middleware for ForkedEvmProvider {
    type Error = ProviderError;
    type Provider = LoopbackProvider;
    type Inner = Self;

    fn inner(&self) -> &Self::Inner {
        unreachable!("There is no inner provider here")
    }

    async fn get_block_number(&self) -> Result<U64, Self::Error> {
        Ok(self.state_block_number.into())
    }

    async fn get_balance<T: Into<NameOrAddress> + Send + Sync>(
        &self,
        from: T,
        _block: Option<BlockId>,
    ) -> Result<U256, ProviderError> {
        let from = match from.into() {
            NameOrAddress::Name(_) => todo!(),
            NameOrAddress::Address(addr) => addr,
        };

        let mut lock = self.backend.lock().await;
        Ok(lock.get_balance(from).await.unwrap())
    }

    async fn get_transaction_count<T: Into<NameOrAddress> + Send + Sync>(
        &self,
        from: T,
        _block: Option<BlockId>,
    ) -> Result<U256, Self::Error> {
        let from = match from.into() {
            NameOrAddress::Name(_) => todo!(),
            NameOrAddress::Address(addr) => addr,
        };

        let mut lock = self.backend.lock().await;
        Ok(lock.get_nonce(from).await.unwrap().into())
    }

    async fn send_transaction<T: Into<TypedTransaction> + Send + Sync>(
        &self,
        tx: T,
        _block: Option<BlockId>,
    ) -> Result<PendingTransaction<'_, Self::Provider>, Self::Error> {
        let tx = tx.into();
        let gas = tx
            .gas()
            .map(|x| i64::try_from(x.as_u64()).unwrap())
            .unwrap_or_default();

        let mut lock = self.backend.lock().await;
        let _ = execute(lock.deref_mut(), &self.header, Revision::London, &tx, gas)
            .await
            .unwrap();

        // TODO:
        Ok(PendingTransaction::new(H256::zero(), &self.dummy_provider))
    }

    async fn call(
        &self,
        tx: &TypedTransaction,
        _block: Option<BlockId>,
    ) -> Result<Bytes, Self::Error> {
        let mut lock = self.backend.lock().await;
        let ret = execute(
            lock.deref_mut(),
            &self.header,
            Revision::London,
            tx,
            i64::MAX,
        )
        .await
        .unwrap();

        // only return the output data if it's successful
        if ret.status_code == StatusCode::Success {
            Ok(ret.output_data.into())
        } else {
            Err(ProviderError::CustomError(format!(
                "reverted with {:?}",
                ret.output_data
            )))
        }
    }

    async fn get_storage_at<T: Into<NameOrAddress> + Send + Sync>(
        &self,
        address: T,
        location: H256,
        _block: Option<BlockId>,
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
