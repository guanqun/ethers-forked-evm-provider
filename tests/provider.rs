use address_literal::addr;
use ethers_forked_evm_provider::ForkedEvmProvider;
use std::sync::Arc;

#[allow(non_snake_case)]
mod IUniswapV2Pair;

#[tokio::test]
async fn test_simple_public_view_functions() {
    let archive_wss_url = std::env::var("ARCHIVE_WSS_URL").expect("failed to get ARCHIVE_WSS_URL");
    let provider = ForkedEvmProvider::new(13458688, &archive_wss_url, "/tmp/sqlite.db")
        .await
        .unwrap();
    let client = Arc::new(provider);

    // pair of WETH-WBTC on uniswap v2
    let v2_pair_contract = IUniswapV2Pair::IUniswapV2Pair::new(
        addr!("0xbb2b8038a1640196fbe3e38816f3e67cba72d940"),
        client.clone(),
    );

    let token0 = v2_pair_contract
        .token_0()
        .call()
        .await
        .map_err(|e| anyhow::anyhow!("failed to get token0: {:?}", e))
        .unwrap();
    let token1 = v2_pair_contract
        .token_1()
        .call()
        .await
        .map_err(|e| anyhow::anyhow!("failed to get token1: {:?}", e))
        .unwrap();

    // WBTC
    assert_eq!(token0, addr!("0x2260fac5e5542a773aa44fbcfedf7c193bc2c599"));
    // WETH
    assert_eq!(token1, addr!("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2"));
}
