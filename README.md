This provider combines dapptools-rs together with ethers and we can easily write unit tests in a forked mode.

There are a few features in mind:

1. It's developed to help verify MEV bots, so the most common use case is to test against forked mode, in order to heavily reduce the number of RPC calls, it would cache the states locally in a sqlite database.
2. Aside from the normal ethers `Middleware` features, it would add some operations that manipulate the chains directly (TODO).

Usage:

```
    use ethers_forked_evm_provider::ForkedEvmProvider;

    let provider = ForkedEvmProvider::new(state_block_number, "wss://your-archive-node-endpoint", db_path)
        .await
        .unwrap();
    let client = Arc::new(provider);
```

If the database path exists, it defaults to pick up the database as the backend, thus no web3 RPC calls would be sent, that would significantly reduce the testing time. (TODO: to show a rough comparision)

If the database path doesn't exist, it would use the web3 RPC calls first, followed by storing these returned values into local sqlite database. Then the next time, your testing process would be super fast.
