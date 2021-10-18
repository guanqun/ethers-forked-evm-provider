use std::fmt::{Debug};
use ethers::providers::{JsonRpcClient, Middleware, ProviderError};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;
use ethers::types::U64;

#[derive(Debug, Clone)]
pub struct ForkedEvmProvider {

}

impl ForkedEvmProvider {
    pub fn new() -> Self {
        Self{}
    }
}

#[async_trait]
impl JsonRpcClient for ForkedEvmProvider {
    type Error = ProviderError;

    async fn request<T, R>(&self, _method: &str, _params: T) -> Result<R, Self::Error> where T: Debug + Serialize + Send + Sync, R: Serialize + DeserializeOwned {
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
}

