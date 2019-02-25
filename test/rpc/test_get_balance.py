import os
import eth_utils
import rlp
from .client import RpcClient, DEFAULT_TX_FEE

import sys
sys.path.append("..")

from test_framework.util import assert_equal, assert_raises_rpc_error, assert_greater_than

class TestGetBalance(RpcClient):
    def test_genesis_account_balance(self):
        addr = self.GENESIS_ADDR
        balance = self.get_balance(addr)
        assert_greater_than(balance, 0)

    def test_address_not_exists(self):
        addr = self.rand_addr()
        balance = self.get_balance(addr)
        assert_equal(0, balance)

    def test_address_empty(self):
        assert_raises_rpc_error(None, None, self.get_balance, "")
        assert_raises_rpc_error(None, None, self.get_balance, "0x")

    def test_address_too_short(self):
        addr = self.rand_addr()
        assert_raises_rpc_error(None, None, self.get_balance, addr[0:-2])

    def test_address_too_long(self):
        addr = self.rand_addr()
        assert_raises_rpc_error(None, None, self.get_balance, addr + "6")

    def test_address_lowercase(self):
        addr = self.rand_addr()
        balance = self.get_balance(addr.lower())
        assert_equal(0, balance)

    def test_address_uppercase(self):
        addr = self.rand_addr()
        balance = self.get_balance("0x" + addr[2:].upper())
        assert_equal(0, balance)

    def test_address_mixedcase(self):
        addr = self.rand_addr()
        addr = addr[0:-1].lower() + "A"
        balance = self.get_balance(addr)
        assert_equal(0, balance)

    def test_balance_after_tx(self):
        addr = self.GENESIS_ADDR
        original_balance = self.get_balance(addr)

        # send a tx to change balance
        tx = self.new_tx(value=789)
        self.send_tx(tx, True)

        # value + gas * price
        cost = 789 + DEFAULT_TX_FEE
        new_balance = self.get_balance(addr)
        assert_equal(original_balance - cost, new_balance)
    
    def test_pivot_chain_changed(self):
        root = self.generate_block()
        original_epoch = self.epoch_number()
        original_balance = self.get_balance(self.GENESIS_ADDR)

        # generate a tx to change the balance
        tx = self.new_tx()
        self.send_tx(tx, True)
        num_blocks = self.epoch_number() - original_epoch
        changed_balance = self.get_balance(self.GENESIS_ADDR)
        assert_greater_than(original_balance, changed_balance)

        # pivot changed without above tx
        parent = root
        for _ in range(0, num_blocks + 1):
            parent = self.generate_block_with_parent(parent, [])
        assert_equal(self.best_block_hash(), parent)
        assert_equal(self.get_balance(self.GENESIS_ADDR), original_balance)
        
        # generate a block on new pivot chain and refer the previous block
        # that contains the above tx
        self.wait_for_receipt(tx.hash_hex())
        assert_equal(self.get_balance(self.GENESIS_ADDR), changed_balance)
        
