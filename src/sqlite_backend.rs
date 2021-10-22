use crate::akula::types::{Account, Incarnation, PartialHeader};
use crate::akula::utils::keccak256;
use bytes::Bytes;
use ethers::types::U256;
use ethers::types::{Address, H256};
use rusqlite::{params, Connection, OpenFlags};
use std::path::Path;
use std::str::FromStr;

#[derive(Debug)]
pub struct SqliteBackend {
    db: Connection,
}

impl SqliteBackend {
    /// Open the sqlite database in read only mode.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("failed to open sqlite dumper");
        Self { db }
    }

    pub fn read_account(&self, address: Address) -> anyhow::Result<Option<Account>> {
        let address_text = hex::encode(address.as_bytes());
        let balance_text: String = self
            .db
            .query_row(
                "SELECT balance FROM balance WHERE address == ?1",
                params![address_text.as_str()],
                |row| row.get(0),
            )
            .map_err(|_| anyhow::anyhow!("failed to get balance"))?;
        let nonce_text: String = self
            .db
            .query_row(
                "SELECT nonce FROM nonce WHERE address == ?1",
                params![address_text.as_str()],
                |row| row.get(0),
            )
            .map_err(|_| anyhow::anyhow!("failed to get nonce"))?;
        let code_hash_text: String = self
            .db
            .query_row(
                "SELECT hash FROM code WHERE address == ?1",
                params![address_text.as_str()],
                |row| row.get(0),
            )
            .map_err(|_| anyhow::anyhow!("failed to get code_hash"))?;

        let balance = U256::from_dec_str(balance_text.as_str())?;
        let nonce = U256::from_dec_str(nonce_text.as_str())?;
        let code_hash = H256::from_str(code_hash_text.as_str())?;

        Ok(Some(Account {
            nonce: nonce.as_u64(),
            balance,
            code_hash,
            incarnation: Default::default(),
        }))
    }

    pub fn read_code(&self, code_hash: H256) -> anyhow::Result<Bytes> {
        let code_hash_text = hex::encode(code_hash.as_bytes());
        let code_text: String = self
            .db
            .query_row(
                "SELECT code FROM code WHERE hash = ?1",
                params![code_hash_text],
                |row| row.get(0),
            )
            .map_err(|_| anyhow::anyhow!("failed to get code hash"))?;
        let code = hex::decode(code_text)?;
        Ok(code.into())
    }

    pub fn read_storage(
        &self,
        address: Address,
        _incarnation: Incarnation,
        location: H256,
    ) -> anyhow::Result<H256> {
        let address_text = hex::encode(address.as_bytes());
        let location_text = hex::encode(location.as_bytes());

        let value_text: String = self
            .db
            .query_row(
                "SELECT value FROM storage WHERE address == ?1 AND slot == ?2",
                params![address_text.as_str(), location_text.as_str()],
                |row| row.get(0),
            )
            .map_err(|_| anyhow::anyhow!("failed to get storage"))?;
        let value = H256::from_str(value_text.as_str()).expect("failed to parse storage");
        Ok(value)
    }

    pub fn read_block_header(&self, block_number: u64) -> anyhow::Result<Option<PartialHeader>> {
        let (hash_text, base_fee_per_gas_text, timestamp, gas_limit, difficulty_text, beneficiary_text): (String, String, u64, u64, String, String) = self.db
            .query_row(
                "SELECT hash, base_fee_per_gas, timestamp, gas_limit, difficulty, beneficiary FROM block WHERE number == ?1",
                params![block_number],
                |row| {
                    Ok((
                        row.get(0).unwrap(),
                        row.get(1).unwrap(),
                        row.get(2).unwrap(),
                        row.get(3).unwrap(),
                        row.get(4).unwrap(),
                        row.get(5).unwrap(),
                    ))
                },
            )
            .map_err(|_| anyhow::anyhow!("failed to get block info"))?;
        let hash = H256::from_str(hash_text.as_str()).unwrap();
        let base_fee_per_gas = U256::from_dec_str(base_fee_per_gas_text.as_str()).unwrap();
        let difficulty = U256::from_dec_str(difficulty_text.as_str()).unwrap();
        let beneficiary = Address::from_str(beneficiary_text.as_str()).unwrap();

        Ok(Some(PartialHeader {
            difficulty,
            number: block_number,
            gas_limit,
            timestamp,
            base_fee_per_gas: Some(base_fee_per_gas.into()),
            hash,
            beneficiary,
        }))
    }
}

#[derive(Debug)]
pub struct SqliteDumper {
    db: Connection,
}

impl SqliteDumper {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let db = Connection::open(path).expect("failed to open sqlite dumper");

        db.execute_batch(r"
            BEGIN;

            DROP TABLE IF EXISTS balance;
            DROP TABLE IF EXISTS nonce;
            DROP TABLE IF EXISTS code;
            DROP TABLE IF EXISTS storage;
            DROP TABLE IF EXISTS block;

            CREATE TABLE balance(address TEXT NOT NULL, balance TEXT NOT NULL);
            CREATE TABLE nonce(address TEXT NOT NULL, nonce TEXT NOT NULL);
            CREATE TABLE code(address TEXT NOT NULL, hash TEXT NOT NULL, code TEXT NOT NULL);
            CREATE TABLE storage(address TEXT NOT NULL, slot TEXT NOT NULL, value TEXT NOT NULL);
            CREATE TABLE block(number INTEGER, hash TEXT NOT NULL, base_fee_per_gas TEXT NOT NULL, timestamp INTEGER, gas_limit INTEGER, difficulty TEXT NOT NULL, beneficiary TEXT NOT NULL);

            COMMIT;
        ").expect("failed to initialize database");

        Self { db }
    }

    pub fn dump_address(&mut self, address: Address, balance: U256, nonce: U256, code: Vec<u8>) {
        let address_text = hex::encode(address.as_bytes());
        let balance_text = format!("{}", balance);
        let nonce_text = format!("{}", nonce);

        let code_hash = keccak256(code.as_slice());
        let code_hash_text = hex::encode(code_hash.as_bytes());
        let code_text = hex::encode(code.as_slice());

        self.db
            .execute(
                "INSERT INTO balance (address, balance) VALUES (?1, ?2)",
                params![address_text.as_str(), balance_text],
            )
            .expect("failed to insert to balance");
        self.db
            .execute(
                "INSERT INTO nonce (address, nonce) VALUES (?1, ?2)",
                params![address_text.as_str(), nonce_text],
            )
            .expect("failed to insert to nonce");
        self.db
            .execute(
                "INSERT INTO code(address, hash, code) VALUES(?1, ?2, ?3)",
                params![address_text.as_str(), code_hash_text, code_text],
            )
            .expect("failed to insert to code");
    }

    pub fn dump_storage(&mut self, address: Address, key: H256, value: H256) {
        let address_text = hex::encode(address.as_bytes());
        let key_text = hex::encode(key.as_bytes());
        let value_text = hex::encode(value.as_bytes());

        self.db
            .execute(
                "INSERT INTO storage(address, slot, value) VALUES(?1, ?2, ?3)",
                params![address_text, key_text, value_text],
            )
            .expect("failed to insert to storage");
    }

    pub fn dump_block_header(
        &mut self,
        block_number: u64,
        hash: H256,
        base_fee_per_gas: U256,
        timestamp: u64,
        gas_limit: u64,
        difficulty: U256,
        beneficiary: Address,
    ) {
        let hash_text = hex::encode(hash.as_bytes());
        let base_fee_per_gas_text = format!("{:?}", base_fee_per_gas);
        let difficulty_text = format!("{:?}", difficulty);
        let beneficiary_text = hex::encode(beneficiary.as_bytes());

        self.db.execute("INSERT INTO block(number, hash, base_fee_per_gas, timestamp, gas_limit, difficulty, beneficiary) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![block_number, hash_text, base_fee_per_gas_text, timestamp, gas_limit, difficulty_text, beneficiary_text]).expect("failed to insert to block header");
    }
}

#[cfg(test)]
mod tests {
    use crate::akula::types::Incarnation;
    use crate::sqlite_backend::{SqliteBackend, SqliteDumper};
    use address_literal::addr;
    use ethers::types::H256;
    use std::str::FromStr;
    use tempfile::tempdir;
    use u256_literal::u256;

    #[tokio::test]
    async fn test_database_write_and_load() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("sqlite.db");

        let rand_hash_1 = H256::random();
        let rand_hash_2 = H256::random();
        let rand_hash_3 = H256::random();

        // save to the file
        {
            let mut dumper = SqliteDumper::new(file_path.clone());

            dumper.dump_address(
                addr!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
                u256!(1234),
                u256!(5678),
                vec![8, 9, 10],
            );
            dumper.dump_storage(
                addr!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
                rand_hash_1,
                rand_hash_2,
            );
            dumper.dump_block_header(
                13330,
                rand_hash_3,
                u256!(6666),
                1239,
                9999,
                u256!(11111122222233333),
                addr!("0x2260fac5e5542a773aa44fbcfedf7c193bc2c599"),
            );
        }

        // load it again
        {
            let backend = SqliteBackend::new(file_path.clone());

            let account = backend
                .read_account(addr!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"))
                .unwrap()
                .unwrap();

            assert_eq!(account.nonce, 5678);
            assert_eq!(account.balance, u256!(1234));
            assert_eq!(
                account.code_hash,
                H256::from_str("13c808d579bcfb9503bd36266832259c3852f41e7d230135f43ab4731b533747")
                    .unwrap()
            );

            let storage = backend
                .read_storage(
                    addr!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"),
                    Incarnation(0),
                    rand_hash_1,
                )
                .unwrap();

            assert_eq!(storage, rand_hash_2);

            let header = backend.read_block_header(13330).unwrap().unwrap();

            assert_eq!(header.hash, rand_hash_3);
            assert_eq!(header.base_fee_per_gas, Some(u256!(6666)));
            assert_eq!(header.timestamp, 1239);
            assert_eq!(header.gas_limit, 9999);
            assert_eq!(header.difficulty, u256!(11111122222233333));
            assert_eq!(
                header.beneficiary,
                addr!("0x2260fac5e5542a773aa44fbcfedf7c193bc2c599")
            );
        }

        dir.close().unwrap();
    }
}
