This provider combines dapptools-rs together with ethers and we can easily write unit tests in a forked mode.

There are a few features in mind:

1. It's developed to help verify MEV bots, so the most common use case is to test against forked mode, in order to heavily reduce the number of RPC calls, it would cache the states locally in a sqlite database (TODO).
2. Aside from the normal ethers `Middleware` features, it would add some operations that manipulate the chains directly.
