#!/usr/bin/env python3
import rlp
from eth_utils import decode_hex
from rlp.sedes import CountableList

from conflux import utils, trie
from conflux.config import default_config
from conflux.messages import BlockHeader, Block
from conflux.transactions import Transaction
from conflux.utils import privtoaddr, int_to_bytes, zpad, encode_hex


def create_block(parent_hash=default_config["GENESIS_PREVHASH"], timestamp=0, difficulty=0, transactions=[]):
    if len(transactions) != 0:
        tx_root = utils.sha3(rlp.encode(transactions))
    else:
        tx_root = trie.BLANK_ROOT
    block = Block(BlockHeader(parent_hash=parent_hash, difficulty=difficulty, timestamp=timestamp,
                              transactions_root=tx_root), transactions=transactions)
    return block


def create_transaction(nonce=0, gas_price=1, gas=100, value=0, receiver=default_config['GENESIS_COINBASE'],
                       v=0, r=0, s=0):
    transaction = Transaction(nonce, gas_price, gas, value, receiver, v, r, s)
    return transaction


def make_genesis():
#     txs = []
#     for i in range(num_txs):
#         sp = decode_hex("46b9e861b63d3509c88b7817275a30d22d62c8cd8fa6486ddee35ef0d8e0495f")
#         addr = privtoaddr(sp)
#         tx = create_transaction(0, 10**15, 200, 10**9, addr)
#         signed_tx = tx.sign(sp)
#         txs.append(signed_tx)
    genesis = create_block()
    return genesis
