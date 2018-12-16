#!/usr/bin/env python3
import rlp
from eth_utils import decode_hex
from rlp.sedes import CountableList

from conflux import utils, trie
from conflux.config import default_config
from conflux.messages import BlockHeader, Block, Transactions
from conflux.transactions import Transaction
from conflux.utils import *
from trie import HexaryTrie

TEST_DIFFICULTY = 4
HASH_MAX = 1 << 256


def create_block(parent_hash=default_config["GENESIS_PREVHASH"], height=0, timestamp=0, difficulty=TEST_DIFFICULTY, transactions=[],
                 gas_limit=0, gas_used=0, referee_hashes=[], author=default_config["GENESIS_COINBASE"]):
    if len(transactions) != 0:
        # tx_root = utils.sha3(rlp.encode(Transactions(transactions)))
        trie_tree = HexaryTrie(db={})
        for i in range(len(transactions)):
            trie_tree[rlp.encode(i)] = rlp.encode(transactions[i])
        tx_root = trie_tree.root_hash
    else:
        tx_root = trie.BLANK_ROOT
    nonce = 0
    while True:
        header = BlockHeader(parent_hash=parent_hash, height=height, difficulty=difficulty, timestamp=timestamp,
                             author=author, transactions_root=tx_root, gas_limit=gas_limit, gas_used=gas_used,
                             referee_hashes=referee_hashes, nonce=nonce)
        if header.pow_decimal() * difficulty < HASH_MAX:
            break
        nonce += 1
    block = Block(block_header=header, transactions=transactions)
    return block


def create_transaction(nonce=0, gas_price=100, gas=1, value=0, receiver=default_config['GENESIS_COINBASE'],
                       data=b'', v=0, r=0, s=0, pri_key=default_config["GENESIS_PRI_KEY"]):
    transaction = Transaction(nonce, gas_price, gas, receiver, value, data, v, r, s)
    return transaction.sign(pri_key)


def make_genesis():
#     txs = []
#     for i in range(num_txs):
#         sp = decode_hex("46b9e861b63d3509c88b7817275a30d22d62c8cd8fa6486ddee35ef0d8e0495f")
#         addr = privtoaddr(sp)
#         tx = create_transaction(0, 10**15, 200, 10**9, addr)
#         signed_tx = tx.sign(sp)
#         txs.append(signed_tx)
    genesis = create_block(difficulty=0)
    return genesis
