use bytes::{Bytes, BytesMut};
use ethers::types::H256;
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
