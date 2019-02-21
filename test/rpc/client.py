import os
import eth_utils
import rlp

import sys
sys.path.append("..")

from conflux.config import default_config
from conflux.transactions import Transaction
from conflux.utils import privtoaddr, sha3_256
from test_framework.test_node import TestNode
from test_framework.util import (
    assert_greater_than, 
    assert_greater_than_or_equal, 
    assert_is_hash_string, 
    wait_until, checktx
)

def genesis_addr() -> str:
    genesis_key = default_config["GENESIS_PRI_KEY"]
    return eth_utils.encode_hex(privtoaddr(genesis_key))

def rand_addr() -> str:
    priv_key = eth_utils.encode_hex(os.urandom(32))
    return eth_utils.encode_hex(privtoaddr(priv_key))

def rand_hash(seed: bytes = None) -> str:
    if seed is None:
        seed = os.urandom(32)
    
    return eth_utils.encode_hex(sha3_256(seed))

ZERO_HASH = eth_utils.encode_hex(b'\x00' * 32)

class RpcClient:
    def get_node(self, idx: int) -> TestNode:
        return self.ctx.nodes[idx]
        
    def node(self) -> TestNode:
        return self.get_node(0)

    def generate_block(self, num_txs: int) -> str:
        assert_greater_than_or_equal(num_txs, 0)
        block_hash = self.node().generateoneblock(num_txs)
        assert_is_hash_string(block_hash)
        return block_hash

    def generate_blocks(self, num_blocks: int, num_txs: int) -> list:
        assert_greater_than(num_blocks, 0)
        assert_greater_than_or_equal(num_txs, 0)

        blocks = []
        for _ in range(0, num_blocks):
            block_hash = self.generate_block(num_txs)
            blocks.append(block_hash)

        return blocks
    
    def generate_block_with_parent(self, parent_hash: str, referee: list, num_txs: int) -> str:
        assert_is_hash_string(parent_hash)

        for r in referee:
            assert_is_hash_string(r)

        assert_greater_than_or_equal(num_txs, 0)

        block_hash = self.node().generatefixedblock(parent_hash, referee, num_txs)
        assert_is_hash_string(block_hash)
        return block_hash
    
    def gas_price(self) -> int:
        return int(self.node().cfx_gasPrice(), 0)

    def epoch_number(self, epoch: str = None) -> int:
        if epoch is None:
            return int(self.node().cfx_epochNumber(), 0)
        else:
            return int(self.node().cfx_epochNumber(epoch), 0)

    def get_balance(self, addr: str) -> int:
        return int(self.node().cfx_getBalance(addr), 0)

    def get_nonce(self, addr: str, epoch: str = None) -> int:
        if epoch is None:
            return int(self.node().cfx_getTransactionCount(addr), 0)
        else:
            return int(self.node().cfx_getTransactionCount(addr, epoch), 0)

    def send_raw_tx(self, raw_tx: str) -> str:
        tx_hash = self.node().cfx_sendRawTransaction(raw_tx)
        assert_is_hash_string(tx_hash)
        return tx_hash

    def send_tx(self, tx: Transaction, wait_for_receipt=False) -> str:
        encoded = eth_utils.encode_hex(rlp.encode(tx))
        tx_hash = self.send_raw_tx(encoded)
        
        if wait_for_receipt:
            self.wait_for_receipt(tx_hash)
        
        return tx_hash

    def wait_for_receipt(self, tx_hash: str, num_txs=1, timeout=60):
        def check_tx():
            self.node().generateoneblock(num_txs)
            return checktx(self.node(), tx_hash)
        wait_until(check_tx, timeout=timeout)

    def block_by_hash(self, block_hash: str, include_txs: bool) -> dict:
        return self.node().cfx_getBlockByHash(block_hash, include_txs)

    def block_by_epoch(self, epoch: str, include_txs: bool) -> dict:
        return self.node().cfx_getBlockByEpochNumber(epoch, include_txs)

    def best_block_hash(self) -> str:
        return self.node().cfx_getBestBlockHash()