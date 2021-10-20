use ethers::types::H256;
use hex_literal::hex;

pub mod address;
pub mod blake2;
pub mod delta;
pub mod evm;
pub mod fee_params;
pub mod interface;
pub mod intra_block_state;
pub mod precompiled;
pub mod types;
pub mod utils;

// Keccak-256 hash of an empty string, KEC("").
pub const EMPTY_HASH: H256 = H256(hex!(
    "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
));

pub fn is_valid_signature(r: H256, s: H256, homestead: bool) -> bool {
    const UPPER: H256 = H256(hex!(
        "fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141"
    ));

    const HALF_N: H256 = H256(hex!(
        "7fffffffffffffffffffffffffffffff5d576e7357a4501ddfe92f46681b20a0"
    ));

    if r.is_zero() || s.is_zero() {
        return false;
    }

    if r >= UPPER && s >= UPPER {
        return false;
    }
    // https://eips.ethereum.org/EIPS/eip-2
    if homestead && s > HALF_N {
        return false;
    }

    true
}
