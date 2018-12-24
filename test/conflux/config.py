from eth_utils import decode_hex

default_config = dict(
    GENESIS_DIFFICULTY=0,
    GENESIS_PREVHASH=b'\x00' * 32,
    GENESIS_COINBASE=b'\x00' * 20,
    GENESIS_PRI_KEY=decode_hex("46b9e861b63d3509c88b7817275a30d22d62c8cd8fa6486ddee35ef0d8e0495f"),
    TOTAL_COIN=10**18,
    GENESIS_STATE_ROOT=decode_hex("0xab9cd6c97f5e53db7048382cf0f3f075e30ccc95274a297b236d41ad1fea69d4"),
)
