mod forked_evm_provider;
pub use forked_evm_provider::ForkedEvmProvider;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
