use bytes::Bytes;
use derive_more::{Deref, DerefMut, Display, From};
use ethers::abi::ethereum_types::H64;
use ethers::types::{Address, H160, H256, U256};
use std::collections::HashMap;
use std::ops::Add;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Log {
    pub address: Address,
    pub topics: Vec<H256>,
    pub data: Bytes,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Account {
    pub nonce: u64,
    pub balance: U256,
    pub code_hash: H256, // hash of the bytecode
    pub incarnation: Incarnation,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
/// Partial header definition without ommers hash and transactions root.
pub struct PartialHeader {
    pub parent_hash: H256,
    pub beneficiary: H160,
    pub state_root: H256,
    pub receipts_root: H256,
    // delete for now
    // pub logs_bloom: Bloom,
    pub difficulty: U256,
    pub number: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub timestamp: u64,
    pub extra_data: Bytes,
    pub mix_hash: H256,
    pub nonce: H64,
    pub base_fee_per_gas: Option<U256>,
}

#[derive(Clone, Debug, Default)]
pub struct Object {
    pub initial: Option<Account>,
    pub current: Option<Account>,
}

#[derive(Debug, Default)]
pub struct CommittedValue {
    /// Value at the begining of the block
    pub initial: H256,
    /// Value at the begining of the transaction; see EIP-2200
    pub original: H256,
}

#[derive(Debug, Default)]
pub struct Storage {
    pub committed: HashMap<H256, CommittedValue>,
    pub current: HashMap<H256, H256>,
}

macro_rules! u64_wrapper {
    ($ty:ident) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            Deref,
            DerefMut,
            Default,
            Display,
            PartialEq,
            Eq,
            From,
            PartialOrd,
            Ord,
            Hash,
        )]
        pub struct $ty(pub u64);

        impl Add<u64> for $ty {
            type Output = Self;

            fn add(self, rhs: u64) -> Self::Output {
                Self(self.0 + rhs)
            }
        }

        // impl Step for $ty {
        //     fn steps_between(start: &Self, end: &Self) -> Option<usize> {
        //         u64::steps_between(&start.0, &end.0)
        //     }
        //
        //     fn forward_checked(start: Self, count: usize) -> Option<Self> {
        //         u64::forward_checked(start.0, count).map(Self)
        //     }
        //
        //     fn backward_checked(start: Self, count: usize) -> Option<Self> {
        //         u64::backward_checked(start.0, count).map(Self)
        //     }
        // }
    };
}

u64_wrapper!(BlockNumber);
u64_wrapper!(Incarnation);
