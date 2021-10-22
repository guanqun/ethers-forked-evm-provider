use crate::akula::interface::State;
use crate::akula::types::{Account, Incarnation, PartialHeader};
use crate::forked_backend::Web3RemoteState;
use crate::sqlite_backend::{SqliteBackend, SqliteDumper};
use async_trait::async_trait;
use bytes::Bytes;
use ethers::abi::ethereum_types::{Address, H256};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

pub enum BackendConfig {
    AllViaWeb3 { wss_url: String },
    TeeWeb3ToLocal { wss_url: String, db_path: PathBuf },
    LocalOnly { db_path: PathBuf },
}

#[derive(Debug)]
pub struct StateMuxer {
    web3: Option<Web3RemoteState>,
    dumper: Option<Arc<Mutex<SqliteDumper>>>,
    db: Option<Arc<Mutex<SqliteBackend>>>,
}

impl StateMuxer {
    pub async fn new(state_block_number: u64, config: BackendConfig) -> anyhow::Result<Self> {
        let this = match config {
            BackendConfig::AllViaWeb3 { wss_url } => Self {
                web3: Some(Web3RemoteState::new(state_block_number, wss_url.as_str()).await?),
                dumper: None,
                db: None,
            },
            BackendConfig::TeeWeb3ToLocal { wss_url, db_path } => Self {
                web3: Some(Web3RemoteState::new(state_block_number, wss_url.as_str()).await?),
                dumper: Some(Arc::new(Mutex::new(SqliteDumper::new(db_path)))),
                db: None,
            },
            BackendConfig::LocalOnly { db_path } => Self {
                web3: None,
                dumper: None,
                db: Some(Arc::new(Mutex::new(SqliteBackend::new(db_path)))),
            },
        };

        Ok(this)
    }
}

#[async_trait]
impl State for StateMuxer {
    async fn read_account(&self, address: Address) -> anyhow::Result<Option<Account>> {
        // if we have db locally, get it!
        if let Some(db) = &self.db {
            let lock = db.lock().await;
            return lock.read_account(address);
        }

        let web3 = self.web3.as_ref().unwrap();
        let ret = web3.read_account(address).await?;

        // write back
        if let Some(dumper) = &self.dumper {
            let mut lock = dumper.lock().await;

            if let Some(account) = &ret {
                let code: Bytes = web3.read_code(account.code_hash).await.unwrap();
                lock.dump_address(
                    address,
                    account.balance,
                    account.nonce.into(),
                    code.to_vec(),
                );
            }
        }

        Ok(ret)
    }

    async fn read_code(&self, code_hash: H256) -> anyhow::Result<Bytes> {
        if let Some(db) = &self.db {
            let lock = db.lock().await;
            return lock.read_code(code_hash);
        }

        let web3 = self.web3.as_ref().unwrap();
        // for this one, we don't need to write it back to database, it's already done in 'read_account()'
        web3.read_code(code_hash).await
    }

    async fn read_storage(
        &self,
        address: Address,
        incarnation: Incarnation,
        location: H256,
    ) -> anyhow::Result<H256> {
        if let Some(db) = &self.db {
            let lock = db.lock().await;
            return lock.read_storage(address, incarnation, location);
        }

        let web3 = self.web3.as_ref().unwrap();
        let ret = web3.read_storage(address, incarnation, location).await?;

        if let Some(dumper) = &self.dumper {
            let mut lock = dumper.lock().await;
            lock.dump_storage(address, location, ret);
        }

        Ok(ret)
    }

    async fn read_block_header(&self, block_number: u64) -> anyhow::Result<Option<PartialHeader>> {
        if let Some(db) = &self.db {
            let lock = db.lock().await;
            return lock.read_block_header(block_number);
        }

        let web3 = self.web3.as_ref().unwrap();
        let ret = web3.read_block_header(block_number).await?;

        if let Some(dumper) = &self.dumper {
            let mut lock = dumper.lock().await;

            if let Some(header) = &ret {
                lock.dump_block_header(
                    block_number,
                    header.hash,
                    header.base_fee_per_gas.unwrap_or_default(),
                    header.timestamp,
                    header.gas_limit,
                    header.difficulty,
                    header.beneficiary,
                );
            }
        }

        Ok(ret)
    }
}
