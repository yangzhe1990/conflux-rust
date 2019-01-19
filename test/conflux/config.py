from eth_utils import decode_hex

default_config = dict(
    GENESIS_DIFFICULTY=0,
    GENESIS_PREVHASH=b'\x00' * 32,
    GENESIS_COINBASE=b'\x00' * 20,
    GENESIS_PRI_KEY=decode_hex("46b9e861b63d3509c88b7817275a30d22d62c8cd8fa6486ddee35ef0d8e0495f"),
    TOTAL_COIN=10**18,
    GENESIS_STATE_ROOT=decode_hex("0x0f7b798e6d8e8843c4a5746f8a41ff6bad6475fa9bf59febbc988ee7851c1514"),
    GENESIS_RECEIPTS_ROOT=decode_hex("0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421"),
)
