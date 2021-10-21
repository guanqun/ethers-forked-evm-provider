use crate::akula::fee_params::param;
use crate::akula::utils::{left_pad, right_pad};
use crate::akula::{blake2, is_valid_signature};
use arrayref::array_ref;
use bytes::{Buf, Bytes};
use ethers::types::*;
use evmodin::Revision;
use num_bigint::BigUint;
use num_traits::Zero;
use ripemd160::*;
use secp256k1::{
    recovery::{RecoverableSignature, RecoveryId},
    Message, SECP256K1,
};
use sha2::*;
use sha3::*;
use std::{
    cmp::min,
    convert::TryFrom,
    io::{repeat, Read},
    mem::size_of,
};
use substrate_bn::*;

pub type GasFunction = fn(Bytes, Revision) -> Option<u64>;
pub type RunFunction = fn(Bytes) -> Option<Bytes>;

pub struct Contract {
    pub gas: GasFunction,
    pub run: RunFunction,
}

pub const CONTRACTS: [Contract; NUM_OF_ISTANBUL_CONTRACTS] = [
    Contract {
        gas: ecrecover_gas,
        run: ecrecover_run,
    },
    Contract {
        gas: sha256_gas,
        run: sha256_run,
    },
    Contract {
        gas: ripemd160_gas,
        run: ripemd160_run,
    },
    Contract {
        gas: id_gas,
        run: id_run,
    },
    Contract {
        gas: expmod_gas,
        run: expmod_run,
    },
    Contract {
        gas: bn_add_gas,
        run: bn_add_run,
    },
    Contract {
        gas: bn_mul_gas,
        run: bn_mul_run,
    },
    Contract {
        gas: snarkv_gas,
        run: snarkv_run,
    },
    Contract {
        gas: blake2_f_gas,
        run: blake2_f_run,
    },
];

pub const NUM_OF_FRONTIER_CONTRACTS: usize = 4;
pub const NUM_OF_BYZANTIUM_CONTRACTS: usize = 8;
pub const NUM_OF_ISTANBUL_CONTRACTS: usize = 9;

fn ecrecover_gas(_: Bytes, _: Revision) -> Option<u64> {
    Some(3_000)
}

fn ecrecover_run_inner(mut input: Bytes) -> Option<Bytes> {
    if input.len() < 128 {
        let mut input2 = input.as_ref().to_vec();
        input2.resize(128, 0);
        input = input2.into();
    }

    let v = U256::from_big_endian(&input[32..64]);
    let r = H256::from_slice(&input[64..96]);
    let s = H256::from_slice(&input[96..128]);

    if !is_valid_signature(r, s, false) {
        return None;
    }

    let mut sig = [0; 64];
    sig[..32].copy_from_slice(&r.0);
    sig[32..].copy_from_slice(&s.0);

    let odd = if v == 28.into() {
        true
    } else if v == 27.into() {
        false
    } else {
        return None;
    };

    let sig =
        RecoverableSignature::from_compact(&sig, RecoveryId::from_i32(odd.into()).ok()?).ok()?;

    let public = &SECP256K1
        .recover(&Message::from_slice(&input[..32]).ok()?, &sig)
        .ok()?;

    let mut out = vec![0; 32];
    out[12..].copy_from_slice(&Keccak256::digest(&public.serialize_uncompressed()[1..])[12..]);

    Some(out.into())
}

fn ecrecover_run(input: Bytes) -> Option<Bytes> {
    Some(ecrecover_run_inner(input).unwrap_or_else(Bytes::new))
}

fn sha256_gas(input: Bytes, _: Revision) -> Option<u64> {
    Some(60 + 12 * ((input.len() as u64 + 31) / 32))
}
fn sha256_run(input: Bytes) -> Option<Bytes> {
    Some(Sha256::digest(&input).to_vec().into())
}

fn ripemd160_gas(input: Bytes, _: Revision) -> Option<u64> {
    Some(600 + 120 * ((input.len() as u64 + 31) / 32))
}
fn ripemd160_run(input: Bytes) -> Option<Bytes> {
    let mut b = [0; 32];
    b[12..].copy_from_slice(&Ripemd160::digest(&input)[..]);
    Some(b.to_vec().into())
}

fn id_gas(input: Bytes, _: Revision) -> Option<u64> {
    Some(15 + 3 * ((input.len() as u64 + 31) / 32))
}
fn id_run(input: Bytes) -> Option<Bytes> {
    Some(input)
}

fn mult_complexity_eip198(x: U256) -> U256 {
    let x_squared = x * x;
    if x <= U256::from(64) {
        x_squared
    } else if x <= U256::from(1024) {
        (x_squared >> 2) + U256::from(96) * x - 3072
    } else {
        (x_squared >> 4) + U256::from(480) * x - 199680
    }
}

fn mult_complexity_eip2565(max_length: U256) -> U256 {
    let words = (max_length + 7) >> 3; // ⌈max_length/8⌉
    words * words
}

fn expmod_gas(mut input: Bytes, rev: Revision) -> Option<u64> {
    let min_gas = if rev < Revision::Berlin { 0 } else { 200 };

    input = right_pad(input, 3 * 32);

    let base_len256 = U256::from_big_endian(&input[0..32]);
    let exp_len256 = U256::from_big_endian(&input[32..64]);
    let mod_len256 = U256::from_big_endian(&input[64..96]);

    if base_len256.is_zero() && mod_len256.is_zero() {
        return Some(min_gas);
    }

    let base_len = usize::try_from(base_len256).ok()?;
    let exp_len = usize::try_from(exp_len256).ok()?;
    u64::try_from(mod_len256).ok()?;

    input.advance(3 * 32);

    let mut exp_head = U256::zero(); // first 32 bytes of the exponent

    if input.len() > base_len {
        let mut exp_input = right_pad(input.slice(base_len..min(base_len + 32, input.len())), 32);
        if exp_len < 32 {
            exp_input = exp_input.slice(..exp_len);
            exp_input = left_pad(exp_input, 32);
        }
        exp_head = U256::from_big_endian(&*exp_input);
    }

    let bit_len = 256 - exp_head.leading_zeros();

    let mut adjusted_exponent_len = U256::zero();
    if exp_len > 32 {
        adjusted_exponent_len = U256::from(8 * (exp_len - 32));
    }
    if bit_len > 1 {
        adjusted_exponent_len += U256::from(bit_len - 1);
    }

    if adjusted_exponent_len.is_zero() {
        adjusted_exponent_len = U256::one();
    }

    let max_length = std::cmp::max(mod_len256, base_len256);

    let gas = {
        if rev < Revision::Berlin {
            mult_complexity_eip198(max_length) * adjusted_exponent_len
                / U256::from(param::G_QUAD_DIVISOR_BYZANTIUM)
        } else {
            mult_complexity_eip2565(max_length) * adjusted_exponent_len
                / U256::from(param::G_QUAD_DIVISOR_BERLIN)
        }
    };

    Some(std::cmp::max(min_gas, u64::try_from(gas).ok()?))
}

fn expmod_run(input: Bytes) -> Option<Bytes> {
    let mut input = right_pad(input, 3 * 32);

    let base_len = usize::try_from(u64::from_be_bytes(*array_ref!(input, 24, 8))).unwrap();
    let exponent_len = usize::try_from(u64::from_be_bytes(*array_ref!(input, 56, 8))).unwrap();
    let modulus_len = usize::try_from(u64::from_be_bytes(*array_ref!(input, 88, 8))).unwrap();

    if modulus_len == 0 {
        return Some(Bytes::new());
    }

    input.advance(96);
    let input = right_pad(input, base_len + exponent_len + modulus_len);

    let base = BigUint::from_bytes_be(&input[..base_len]);
    let exponent = BigUint::from_bytes_be(&input[base_len..base_len + exponent_len]);
    let modulus = BigUint::from_bytes_be(
        &input[base_len + exponent_len..base_len + exponent_len + modulus_len],
    );

    let mut out = vec![0; modulus_len];
    if modulus.is_zero() {
        return Some(out.into());
    }

    let b = base.modpow(&exponent, &modulus).to_bytes_be();

    out[modulus_len - b.len()..].copy_from_slice(&b);

    Some(out.into())
}

fn bn_add_gas(_: Bytes, rev: Revision) -> Option<u64> {
    Some({
        if rev >= Revision::Istanbul {
            150
        } else {
            500
        }
    })
}

fn parse_fr_point(r: &mut impl Read) -> Option<substrate_bn::Fr> {
    let mut buf = [0; 32];

    r.read_exact(&mut buf[..]).ok()?;
    substrate_bn::Fr::from_slice(&buf).ok()
}

fn parse_bn_point(r: &mut impl Read) -> Option<substrate_bn::G1> {
    use substrate_bn::*;

    let mut buf = [0; 32];

    r.read_exact(&mut buf).unwrap();
    let x = Fq::from_slice(&buf[..]).ok()?;

    r.read_exact(&mut buf).unwrap();
    let y = Fq::from_slice(&buf[..]).ok()?;

    Some({
        if x.is_zero() && y.is_zero() {
            G1::zero()
        } else {
            AffineG1::new(x, y).ok()?.into()
        }
    })
}

fn bn_add_run(input: Bytes) -> Option<Bytes> {
    let mut input = Read::chain(input.as_ref(), repeat(0));

    let a = parse_bn_point(&mut input)?;
    let b = parse_bn_point(&mut input)?;

    let mut out = [0u8; 64];
    if let Some(sum) = AffineG1::from_jacobian(a + b) {
        sum.x().to_big_endian(&mut out[..32]).unwrap();
        sum.y().to_big_endian(&mut out[32..]).unwrap();
    }

    Some(out.to_vec().into())
}

fn bn_mul_gas(_: Bytes, rev: Revision) -> Option<u64> {
    Some({
        if rev >= Revision::Istanbul {
            6_000
        } else {
            40_000
        }
    })
}
fn bn_mul_run(input: Bytes) -> Option<Bytes> {
    let mut input = Read::chain(input.as_ref(), repeat(0));

    let a = parse_bn_point(&mut input)?;
    let b = parse_fr_point(&mut input)?;

    let mut out = [0u8; 64];
    if let Some(product) = AffineG1::from_jacobian(a * b) {
        product.x().to_big_endian(&mut out[..32]).unwrap();
        product.y().to_big_endian(&mut out[32..]).unwrap();
    }

    Some(out.to_vec().into())
}

const SNARKV_STRIDE: u8 = 192;

fn snarkv_gas(input: Bytes, rev: Revision) -> Option<u64> {
    let k = input.len() as u64 / SNARKV_STRIDE as u64;
    Some({
        if rev >= Revision::Istanbul {
            34_000 * k + 45_000
        } else {
            80_000 * k + 100_000
        }
    })
}
fn snarkv_run(input: Bytes) -> Option<Bytes> {
    if input.len() % usize::from(SNARKV_STRIDE) != 0 {
        return None;
    }

    let k = input.len() / usize::from(SNARKV_STRIDE);

    let ret_val = if input.is_empty() {
        U256::one()
    } else {
        let mut mul = Gt::one();
        for i in 0..k {
            let a_x = Fq::from_slice(&input[i * 192..i * 192 + 32]).ok()?;
            let a_y = Fq::from_slice(&input[i * 192 + 32..i * 192 + 64]).ok()?;
            let b_a_y = Fq::from_slice(&input[i * 192 + 64..i * 192 + 96]).ok()?;
            let b_a_x = Fq::from_slice(&input[i * 192 + 96..i * 192 + 128]).ok()?;
            let b_b_y = Fq::from_slice(&input[i * 192 + 128..i * 192 + 160]).ok()?;
            let b_b_x = Fq::from_slice(&input[i * 192 + 160..i * 192 + 192]).ok()?;

            let b_a = Fq2::new(b_a_x, b_a_y);
            let b_b = Fq2::new(b_b_x, b_b_y);
            let b = if b_a.is_zero() && b_b.is_zero() {
                G2::zero()
            } else {
                G2::from(AffineG2::new(b_a, b_b).ok()?)
            };
            let a = if a_x.is_zero() && a_y.is_zero() {
                G1::zero()
            } else {
                G1::from(AffineG1::new(a_x, a_y).ok()?)
            };
            mul = mul * pairing(a, b);
        }

        if mul == Gt::one() {
            U256::one()
        } else {
            U256::zero()
        }
    };

    let mut buf = [0; 32];
    ret_val.to_big_endian(&mut buf);
    Some(buf.to_vec().into())
}

fn blake2_f_gas(input: Bytes, _: Revision) -> Option<u64> {
    if input.len() < 4 {
        // blake2_f_run will fail anyway
        return Some(0);
    }
    Some(u32::from_be_bytes(*array_ref!(input, 0, 4)).into())
}

fn blake2_f_run(input: Bytes) -> Option<Bytes> {
    const BLAKE2_F_ARG_LEN: usize = 213;

    if input.len() != BLAKE2_F_ARG_LEN {
        return None;
    }

    let mut rounds_buf: [u8; 4] = [0; 4];
    rounds_buf.copy_from_slice(&input[0..4]);
    let rounds: u32 = u32::from_be_bytes(rounds_buf);

    // we use from_le_bytes below to effectively swap byte order to LE if architecture is BE

    let mut h_buf: [u8; 64] = [0; 64];
    h_buf.copy_from_slice(&input[4..68]);
    let mut h = [0u64; 8];
    let mut ctr = 0;
    for state_word in &mut h {
        let mut temp: [u8; 8] = Default::default();
        temp.copy_from_slice(&h_buf[(ctr * 8)..(ctr + 1) * 8]);
        *state_word = u64::from_le_bytes(temp);
        ctr += 1;
    }

    let mut m_buf: [u8; 128] = [0; 128];
    m_buf.copy_from_slice(&input[68..196]);
    let mut m = [0u64; 16];
    ctr = 0;
    for msg_word in &mut m {
        let mut temp: [u8; 8] = Default::default();
        temp.copy_from_slice(&m_buf[(ctr * 8)..(ctr + 1) * 8]);
        *msg_word = u64::from_le_bytes(temp);
        ctr += 1;
    }

    let mut t_0_buf: [u8; 8] = [0; 8];
    t_0_buf.copy_from_slice(&input[196..204]);
    let t_0 = u64::from_le_bytes(t_0_buf);

    let mut t_1_buf: [u8; 8] = [0; 8];
    t_1_buf.copy_from_slice(&input[204..212]);
    let t_1 = u64::from_le_bytes(t_1_buf);

    let f = if input[212] == 1 {
        true
    } else if input[212] == 0 {
        false
    } else {
        return None;
    };

    blake2::compress(&mut h, m, [t_0, t_1], f, rounds as usize);

    let mut output_buf = [0u8; 8 * size_of::<u64>()];
    for (i, state_word) in h.iter().enumerate() {
        output_buf[i * 8..(i + 1) * 8].copy_from_slice(&state_word.to_le_bytes());
    }

    Some(output_buf.to_vec().into())
}