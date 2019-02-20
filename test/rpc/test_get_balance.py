import os
import eth_utils
import rlp
from .client import RpcClient, genesis_addr, rand_addr

import sys
sys.path.append("..")

from test_framework.blocktools import create_transaction
from test_framework.util import assert_equal, assert_raises_rpc_error, assert_greater_than
    
class TestGetBalance(RpcClient):
    def test_genesis_account_balance(self):
        addr = genesis_addr()
        balance = self.get_balance(addr)
        assert_greater_than(balance, 0)

    def test_address_not_exists(self):
        addr = rand_addr()
        balance = self.get_balance(addr)
        assert_equal(0, balance)

    def test_address_empty(self):
        assert_raises_rpc_error(None, None, self.get_balance, "")
        assert_raises_rpc_error(None, None, self.get_balance, "0x")

    def test_address_too_short(self):
        addr = rand_addr()
        assert_raises_rpc_error(None, None, self.get_balance, addr[0:-2])

    def test_address_too_long(self):
        addr = rand_addr()
        assert_raises_rpc_error(None, None, self.get_balance, addr + "6")

    def test_address_lowercase(self):
        addr = rand_addr()
        balance = self.get_balance(addr.lower())
        assert_equal(0, balance)

    def test_address_uppercase(self):
        addr = rand_addr()
        balance = self.get_balance("0x" + addr[2:].upper())
        assert_equal(0, balance)

    def test_address_mixedcase(self):
        addr = rand_addr()
        addr = addr[0:-1].lower() + "A"
        balance = self.get_balance(addr)
        assert_equal(0, balance)

    def test_balance_after_tx(self):
        addr = genesis_addr()
        original_balance = self.get_balance(addr)
        original_nonce = self.get_nonce(addr)

        # send a tx to change balance
        tx = create_transaction(original_nonce, value=1000)
        self.send_tx(tx, True)

        # value + gas * price
        cost = 1000 + 21000*1
        new_balance = self.get_balance(addr)
        assert_equal(original_balance - cost, new_balance)

