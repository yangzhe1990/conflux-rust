from eth_utils import decode_hex

default_config = dict(
    GENESIS_DIFFICULTY=0,
    GENESIS_PREVHASH=b'\x00' * 32,
    GENESIS_COINBASE=b'\x00' * 20,
    GENESIS_PRI_KEY=decode_hex("46b9e861b63d3509c88b7817275a30d22d62c8cd8fa6486ddee35ef0d8e0495f"),
    TOTAL_COIN=10**18,
    GENESIS_STATE_ROOT=decode_hex("0x351659ee1319ca5eb31075a2c88685e48c282a2121815cad67237e74018ea881"),
)
