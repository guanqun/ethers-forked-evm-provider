use ethers::providers::Middleware;
use ethers_forked_evm_provider::ForkedEvmProvider;

#[tokio::test]
async fn test_forked_evm_provider() {
    let provider = ForkedEvmProvider::new();
    let block_number = provider.get_block_number().await.unwrap();
    assert_eq!(block_number, 1234.into());
}
