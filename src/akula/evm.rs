use crate::akula::address::{create2_address, create_address};
use crate::akula::fee_params::{fee, param};
use crate::akula::interface::State;
use crate::akula::intra_block_state::IntraBlockState;
use crate::akula::types::{Log, PartialHeader};
use crate::akula::utils::{get_effective_gas_price, get_sender};
use crate::akula::{precompiled, EMPTY_HASH};
use async_recursion::async_recursion;
use bytes::Bytes;
use ethers::types::transaction::eip2718::TypedTransaction;
use ethers::types::{Address, NameOrAddress, H256, U256};
use evmodin::{
    continuation::{interrupt::*, interrupt_data::*, resume_data::*, Interrupt},
    host::*,
    CallKind, CreateMessage, Message, Output, Revision, StatusCode,
};
use sha3::{Digest, Keccak256};
use std::{cmp::min, convert::TryFrom};

pub const ADDRESS_LENGTH: usize = Address::len_bytes();

#[derive(Debug)]
pub struct CallResult {
    /// EVM exited with this status code.
    pub status_code: StatusCode,
    /// How much gas was left after execution
    pub gas_left: i64,
    /// Output data returned.
    pub output_data: Bytes,
    /// Only valid when it's create message
    pub create_address: Option<Address>,
}

struct Evm<'state, 'h, 't, B>
where
    B: State,
{
    state: &'state mut IntraBlockState<B>,
    header: &'h PartialHeader,
    revision: Revision,
    txn: &'t TypedTransaction,
    beneficiary: Address,
}

pub async fn execute<B: State>(
    state: &mut IntraBlockState<B>,
    header: &PartialHeader,
    revision: Revision,
    txn: &TypedTransaction,
    gas: i64,
) -> anyhow::Result<CallResult> {
    let mut evm = Evm {
        header,
        state,
        revision,
        txn,
        beneficiary: header.beneficiary,
    };

    let from = txn.from().cloned().unwrap_or_default();

    let to = txn.to().map(|x| match x {
        NameOrAddress::Name(_) => {
            todo!()
        }
        NameOrAddress::Address(address) => address.clone(),
    });

    let input_data = txn.data().map(|x| x.0.clone()).unwrap_or_default();
    let value = txn.value().cloned().unwrap_or_default();

    let res = if let Some(to) = to {
        evm.call(Message {
            kind: CallKind::Call,
            is_static: from.is_zero(),
            depth: 0,
            sender: from,
            input_data,
            value,
            gas,
            recipient: to,
            code_address: to,
        })
        .await?
    } else {
        evm.create(CreateMessage {
            depth: 0,
            gas,
            sender: from,
            initcode: input_data,
            endowment: value,
            salt: None,
        })
        .await?
    };

    Ok(CallResult {
        status_code: res.status_code,
        gas_left: res.gas_left,
        output_data: res.output_data,
        create_address: res.create_address,
    })
}

impl<'state, 'h, 't, B> Evm<'state, 'h, 't, B>
where
    B: State,
{
    #[async_recursion]
    async fn create(&mut self, message: CreateMessage) -> anyhow::Result<Output> {
        let mut res = Output {
            status_code: StatusCode::Success,
            gas_left: message.gas,
            output_data: Bytes::new(),
            create_address: None,
        };

        let value = message.endowment;
        if self.state.get_balance(message.sender).await? < value {
            res.status_code = StatusCode::InsufficientBalance;
            return Ok(res);
        }

        let nonce = self.state.get_nonce(message.sender).await?;
        self.state.set_nonce(message.sender, nonce + 1).await?;

        let contract_addr = {
            if let Some(salt) = message.salt {
                create2_address(
                    message.sender,
                    salt,
                    H256::from_slice(&Keccak256::digest(&message.initcode[..])[..]),
                )
            } else {
                create_address(message.sender, nonce)
            }
        };

        self.state.access_account(contract_addr);

        if self.state.get_nonce(contract_addr).await? != 0
            || self.state.get_code_hash(contract_addr).await? != EMPTY_HASH
        {
            // https://github.com/ethereum/EIPs/issues/684
            res.status_code = StatusCode::InvalidInstruction;
            res.gas_left = 0;
            return Ok(res);
        }

        let snapshot = self.state.take_snapshot();

        self.state.create_contract(contract_addr).await?;

        if self.revision >= Revision::Spurious {
            self.state.set_nonce(contract_addr, 1).await?;
        }

        self.state
            .subtract_from_balance(message.sender, value)
            .await?;
        self.state.add_to_balance(contract_addr, value).await?;

        let deploy_message = Message {
            kind: CallKind::Call,
            is_static: false,
            depth: message.depth,
            gas: message.gas,
            recipient: contract_addr,
            code_address: Address::zero(),
            sender: message.sender,
            input_data: Default::default(),
            value: message.endowment,
        };

        res = self
            .execute(deploy_message, message.initcode.as_ref().to_vec())
            .await?;

        if res.status_code == StatusCode::Success {
            let code_len = res.output_data.len();
            let code_deploy_gas = code_len as u64 * fee::G_CODE_DEPOSIT;

            if self.revision >= Revision::London && code_len > 0 && res.output_data[0] == 0xEF {
                // https://eips.ethereum.org/EIPS/eip-3541
                res.status_code = StatusCode::ContractValidationFailure;
            } else if self.revision >= Revision::Spurious && code_len > param::MAX_CODE_SIZE {
                // https://eips.ethereum.org/EIPS/eip-170
                res.status_code = StatusCode::OutOfGas;
            } else if res.gas_left >= 0 && res.gas_left as u64 >= code_deploy_gas {
                res.gas_left -= code_deploy_gas as i64;
                self.state
                    .set_code(contract_addr, res.output_data.clone())
                    .await?;
            } else if self.revision >= Revision::Homestead {
                res.status_code = StatusCode::OutOfGas;
            }
        }

        if res.status_code == StatusCode::Success {
            res.create_address = Some(contract_addr);
        } else {
            self.state.revert_to_snapshot(snapshot);
            if res.status_code != StatusCode::Revert {
                res.gas_left = 0;
            }
        }

        Ok(res)
    }

    #[async_recursion]
    async fn call(&mut self, message: Message) -> anyhow::Result<Output> {
        let mut res = Output {
            status_code: StatusCode::Success,
            gas_left: message.gas,
            output_data: Bytes::new(),
            create_address: None,
        };

        let value = message.value;
        if message.kind != CallKind::DelegateCall
            && self.state.get_balance(message.sender).await? < value
        {
            res.status_code = StatusCode::InsufficientBalance;
            return Ok(res);
        }

        let precompiled = self.is_precompiled(message.code_address);

        // https://eips.ethereum.org/EIPS/eip-161
        if value.is_zero()
            && self.revision >= Revision::Spurious
            && !precompiled
            && !self.state.exists(message.code_address).await?
        {
            return Ok(res);
        }

        let snapshot = self.state.take_snapshot();

        if message.kind == CallKind::Call {
            if message.is_static {
                // Match geth logic
                // https://github.com/ethereum/go-ethereum/blob/v1.9.25/core/vm/evm.go#L391
                self.state.touch(message.recipient);
            } else {
                self.state
                    .subtract_from_balance(message.sender, value)
                    .await?;
                self.state.add_to_balance(message.recipient, value).await?;
            }
        }

        if precompiled {
            let num = message.code_address.0[ADDRESS_LENGTH - 1] as usize;
            let contract = &precompiled::CONTRACTS[num - 1];
            let input = message.input_data;
            if let Some(gas) =
                (contract.gas)(input.clone(), self.revision).and_then(|g| i64::try_from(g).ok())
            {
                if gas > message.gas {
                    res.status_code = StatusCode::OutOfGas;
                } else if let Some(output) = (contract.run)(input) {
                    res.status_code = StatusCode::Success;
                    res.gas_left = message.gas - gas;
                    res.output_data = output;
                } else {
                    res.status_code = StatusCode::PrecompileFailure;
                }
            } else {
                res.status_code = StatusCode::OutOfGas;
            }
        } else {
            let code = self
                .state
                .get_code(message.code_address)
                .await?
                .unwrap_or_default();
            if code.is_empty() {
                return Ok(res);
            }

            res = self.execute(message, code.as_ref().to_vec()).await?;
        }

        if res.status_code != StatusCode::Success {
            self.state.revert_to_snapshot(snapshot);
            if res.status_code != StatusCode::Revert {
                res.gas_left = 0;
            }
        }

        Ok(res)
    }

    async fn execute(&mut self, msg: Message, code: Vec<u8>) -> anyhow::Result<Output> {
        let mut interrupt = evmodin::AnalyzedCode::analyze(code)
            .execute_resumable(false, msg, self.revision)
            .resume(());

        let output = loop {
            interrupt = match interrupt {
                InterruptVariant::InstructionStart(_) => unreachable!("tracing is disabled"),
                InterruptVariant::AccountExists(i) => {
                    let address = i.data().address;
                    let exists = if self.revision >= Revision::Spurious {
                        !self.state.is_dead(address).await?
                    } else {
                        self.state.exists(address).await?
                    };
                    i.resume(AccountExistsStatus { exists })
                }
                InterruptVariant::GetBalance(i) => {
                    let balance = self.state.get_balance(i.data().address).await?;
                    i.resume(Balance { balance })
                }
                InterruptVariant::GetCodeSize(i) => {
                    let code_size = self
                        .state
                        .get_code(i.data().address)
                        .await?
                        .map(|c| c.len())
                        .unwrap_or(0)
                        .into();
                    i.resume(CodeSize { code_size })
                }
                InterruptVariant::GetStorage(i) => {
                    let value = self
                        .state
                        .get_current_storage(i.data().address, i.data().key)
                        .await?;
                    i.resume(StorageValue { value })
                }
                InterruptVariant::SetStorage(i) => {
                    let &SetStorage {
                        address,
                        key,
                        value: new_val,
                    } = i.data();

                    let current_val = self.state.get_current_storage(address, key).await?;

                    let status = if current_val == new_val {
                        StorageStatus::Unchanged
                    } else {
                        self.state.set_storage(address, key, new_val).await?;

                        let eip1283 = self.revision >= Revision::Istanbul
                            || self.revision == Revision::Constantinople;

                        if !eip1283 {
                            if current_val.is_zero() {
                                StorageStatus::Added
                            } else if new_val.is_zero() {
                                self.state.add_refund(fee::R_SCLEAR);
                                StorageStatus::Deleted
                            } else {
                                StorageStatus::Modified
                            }
                        } else {
                            let sload_cost = {
                                if self.revision >= Revision::Berlin {
                                    fee::WARM_STORAGE_READ_COST
                                } else if self.revision >= Revision::Istanbul {
                                    fee::G_SLOAD_ISTANBUL
                                } else {
                                    fee::G_SLOAD_TANGERINE_WHISTLE
                                }
                            };

                            let mut sstore_reset_gas = fee::G_SRESET;
                            if self.revision >= Revision::Berlin {
                                sstore_reset_gas -= fee::COLD_SLOAD_COST;
                            }

                            // https://eips.ethereum.org/EIPS/eip-1283
                            let original_val =
                                self.state.get_original_storage(address, key).await?;

                            // https://eips.ethereum.org/EIPS/eip-3529
                            let sstore_clears_refund = if self.revision >= Revision::London {
                                sstore_reset_gas + fee::ACCESS_LIST_STORAGE_KEY_COST
                            } else {
                                fee::R_SCLEAR
                            };

                            if original_val == current_val {
                                if original_val.is_zero() {
                                    StorageStatus::Added
                                } else {
                                    if new_val.is_zero() {
                                        self.state.add_refund(sstore_clears_refund);
                                    }
                                    StorageStatus::Modified
                                }
                            } else {
                                if !original_val.is_zero() {
                                    if current_val.is_zero() {
                                        self.state.subtract_refund(sstore_clears_refund);
                                    }
                                    if new_val.is_zero() {
                                        self.state.add_refund(sstore_clears_refund);
                                    }
                                }
                                if original_val == new_val {
                                    let refund = {
                                        if original_val.is_zero() {
                                            fee::G_SSET - sload_cost
                                        } else {
                                            sstore_reset_gas - sload_cost
                                        }
                                    };

                                    self.state.add_refund(refund);
                                }
                                StorageStatus::ModifiedAgain
                            }
                        }
                    };

                    i.resume(StorageStatusInfo { status })
                }
                InterruptVariant::GetCodeHash(i) => {
                    let address = i.data().address;
                    let hash = {
                        if self.state.is_dead(address).await? {
                            H256::zero()
                        } else {
                            self.state.get_code_hash(address).await?
                        }
                    };
                    i.resume(CodeHash { hash })
                }
                InterruptVariant::CopyCode(i) => {
                    let &CopyCode {
                        address,
                        offset,
                        max_size,
                    } = i.data();

                    let mut buffer = vec![0; max_size];

                    let code = self.state.get_code(address).await?.unwrap_or_default();

                    let mut copied = 0;
                    if offset < code.len() {
                        copied = min(max_size, code.len() - offset);
                        buffer[..copied].copy_from_slice(&code[offset..offset + copied]);
                    }

                    buffer.truncate(copied);
                    let code = buffer.into();

                    i.resume(Code { code })
                }
                InterruptVariant::Selfdestruct(i) => {
                    self.state.record_selfdestruct(i.data().address);
                    let balance = self.state.get_balance(i.data().address).await?;
                    self.state
                        .add_to_balance(i.data().beneficiary, balance)
                        .await?;
                    self.state.set_balance(i.data().address, 0).await?;

                    i.resume(())
                }
                InterruptVariant::Call(i) => {
                    let output = match i.data() {
                        Call::Create(message) => {
                            let res = self.create(message.clone()).await?;

                            // https://eips.ethereum.org/EIPS/eip-211
                            if res.status_code == StatusCode::Revert {
                                // geth returns CREATE output only in case of REVERT
                                res
                            } else {
                                Output {
                                    output_data: Default::default(),
                                    ..res
                                }
                            }
                        }
                        Call::Call(message) => self.call(message.clone()).await?,
                    };

                    i.resume(CallOutput { output })
                }
                InterruptVariant::GetTxContext(i) => {
                    let base_fee_per_gas = self.header.base_fee_per_gas.unwrap_or_else(U256::zero);
                    let tx_gas_price = get_effective_gas_price(&self.txn, base_fee_per_gas);
                    let tx_origin = get_sender(&self.txn);
                    let block_coinbase = self.beneficiary;
                    let block_number = self.header.number;
                    let block_timestamp = self.header.timestamp;
                    let block_gas_limit = self.header.gas_limit;
                    let block_difficulty = self.header.difficulty;
                    let chain_id = 1.into();
                    let block_base_fee = base_fee_per_gas;

                    let context = TxContext {
                        tx_gas_price,
                        tx_origin,
                        block_coinbase,
                        block_number,
                        block_timestamp,
                        block_gas_limit,
                        block_difficulty,
                        chain_id,
                        block_base_fee,
                    };

                    i.resume(TxContextData { context })
                }
                InterruptVariant::GetBlockHash(i) => {
                    let n = i.data().block_number;

                    let base_number = self.header.number;
                    let distance = base_number - n;
                    assert!(distance <= 256);

                    let hash = self.state.db().read_block_header(n).await?.unwrap().hash;

                    i.resume(BlockHash { hash })
                }
                InterruptVariant::EmitLog(i) => {
                    self.state.add_log(Log {
                        address: i.data().address,
                        topics: i.data().topics.as_slice().into(),
                        data: i.data().data.clone(),
                    });

                    i.resume(())
                }
                InterruptVariant::AccessAccount(i) => {
                    let address = i.data().address;

                    let status = if self.is_precompiled(address) {
                        AccessStatus::Warm
                    } else {
                        self.state.access_account(address)
                    };
                    i.resume(AccessAccountStatus { status })
                }
                InterruptVariant::AccessStorage(i) => {
                    let status = self.state.access_storage(i.data().address, i.data().key);
                    i.resume(AccessStorageStatus { status })
                }
                InterruptVariant::Complete(i) => {
                    let output = match i {
                        Ok(output) => output.into(),
                        Err(status_code) => Output {
                            status_code,
                            gas_left: 0,
                            output_data: bytes::Bytes::new(),
                            create_address: None,
                        },
                    };

                    break output;
                }
            };
        };

        Ok(output)
    }

    fn number_of_precompiles(&self) -> u8 {
        match self.revision {
            Revision::Frontier | Revision::Homestead | Revision::Tangerine | Revision::Spurious => {
                precompiled::NUM_OF_FRONTIER_CONTRACTS as u8
            }
            Revision::Byzantium | Revision::Constantinople | Revision::Petersburg => {
                precompiled::NUM_OF_BYZANTIUM_CONTRACTS as u8
            }
            Revision::Istanbul | Revision::Berlin | Revision::London | Revision::Shanghai => {
                precompiled::NUM_OF_ISTANBUL_CONTRACTS as u8
            }
        }
    }

    fn is_precompiled(&self, contract: Address) -> bool {
        if contract.is_zero() {
            false
        } else {
            let mut max_precompiled = Address::zero();
            max_precompiled.0[ADDRESS_LENGTH - 1] = self.number_of_precompiles() as u8;
            contract <= max_precompiled
        }
    }
}
