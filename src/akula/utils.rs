use bytes::{Bytes, BytesMut};
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::types::{Address, H256, U256};
use sha3::{Digest, Keccak256};

pub fn keccak256(data: impl AsRef<[u8]>) -> H256 {
    H256::from_slice(&Keccak256::digest(data.as_ref()))
}

fn pad<const LEFT: bool>(buffer: Bytes, min_size: usize) -> Bytes {
    if buffer.len() >= min_size {
        return buffer;
    }

    let point = if LEFT { min_size - buffer.len() } else { 0 };

    let mut b = BytesMut::with_capacity(min_size);
    b.resize(min_size, 0);
    b[point..point + buffer.len()].copy_from_slice(&buffer[..]);
    b.freeze()
}

pub fn left_pad(buffer: Bytes, min_size: usize) -> Bytes {
    pad::<true>(buffer, min_size)
}

pub fn right_pad(buffer: Bytes, min_size: usize) -> Bytes {
    pad::<false>(buffer, min_size)
}

pub fn get_effective_gas_price(tx: &TypedTransaction, base_fee_per_gas: U256) -> U256 {
    match tx {
        TypedTransaction::Legacy(tx) => tx.gas_price.unwrap_or_default(),
        TypedTransaction::Eip2930(tx) => tx.tx.gas_price.unwrap_or_default(),
        TypedTransaction::Eip1559(tx) => {
            if let Some(max_fee_per_gas) = tx.max_fee_per_gas {
                assert!(max_fee_per_gas >= base_fee_per_gas);
                let priority_gas_fee = std::cmp::min(
                    tx.max_priority_fee_per_gas.unwrap_or_default(),
                    max_fee_per_gas - base_fee_per_gas,
                );
                priority_gas_fee + base_fee_per_gas
            } else {
                // just query calls
                U256::zero()
            }
        }
    }
}

pub fn get_sender(tx: &TypedTransaction) -> Address {
    tx.from().cloned().unwrap_or_default()
}
