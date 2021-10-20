use akula::fee_params::*;
// pub use evm::call;
pub use forked_evm_provider::ForkedEvmProvider;

pub mod akula;
mod forked_backend;
mod forked_evm_provider;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
