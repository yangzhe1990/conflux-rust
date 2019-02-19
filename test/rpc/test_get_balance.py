import os
import sys
import eth_utils
import rlp
sys.path.append("..")
from conflux.utils import privtoaddr
from conflux.config import default_config
from test_framework.blocktools import create_transaction
from test_framework.util import assert_equal, assert_raises_rpc_error, wait_until, checktx

def _genesis_addr() -> str:
    genesis_key = default_config["GENESIS_PRI_KEY"]
    return eth_utils.encode_hex(privtoaddr(genesis_key))

def _rand_addr() -> str:
    priv_key = eth_utils.encode_hex(os.urandom(32))
    return eth_utils.encode_hex(privtoaddr(priv_key))
    
class TestGetBalance:
    def _get_balance(self, addr: str) -> int:
        res = self.ctx.nodes[0].cfx_getBalance(addr)
        return int(res, 0)

    def _get_nonce(self, addr: str) -> int:
        res = self.ctx.nodes[0].cfx_getTransactionCount(addr)
        return int(res, 0)

    def _send_tx(self, tx) -> str:
        encoded = eth_utils.encode_hex(rlp.encode(tx))
        return self.ctx.nodes[0].cfx_sendRawTransaction(encoded)

    def _wait_for_tx(self, hash):
        def check_tx():
            self.ctx.nodes[0].generateoneblock(1)
            return checktx(self.ctx.nodes[0], hash)
        wait_until(check_tx, timeout=5)

    def test_genesis_account_balance(self):
        addr = _genesis_addr()
        balance = self._get_balance(addr)
        assert_equal(default_config["TOTAL_COIN"], balance)

    def test_address_not_exists(self):
        addr = _rand_addr()
        balance = self._get_balance(addr)
        assert_equal(0, balance)

    def test_address_empty(self):
        assert_raises_rpc_error(None, None, self._get_balance, "")
        assert_raises_rpc_error(None, None, self._get_balance, "0x")

    def test_address_too_short(self):
        addr = _rand_addr()
        assert_raises_rpc_error(None, None, self._get_balance, addr[0:-2])

    def test_address_too_long(self):
        addr = _rand_addr()
        assert_raises_rpc_error(None, None, self._get_balance, addr + "6")

    def test_address_lowercase(self):
        addr = _rand_addr()
        balance = self._get_balance(addr.lower())
        assert_equal(0, balance)

    def test_address_uppercase(self):
        addr = _rand_addr()
        balance = self._get_balance("0x" + addr[2:].upper())
        assert_equal(0, balance)

    def test_address_mixedcase(self):
        addr = _rand_addr()
        addr = addr[0:-1].lower() + "A"
        balance = self._get_balance(addr)
        assert_equal(0, balance)

    #FIXME remove the prefix '_' to enable this test case
    # Currently, the sent tx in txpool is always pending.
    def _test_balance_after_tx(self):
        addr = _genesis_addr()
        original_balance = self._get_balance(addr)
        original_nonce = self._get_nonce(addr)

        # send a tx to change balance        
        tx = create_transaction(original_nonce, value=1000)
        tx_hash = self._send_tx(tx)

        self._wait_for_tx(tx_hash)

        # value + gas * price
        cost = 1000 + 21000*1
        new_balance = self._get_balance(addr)
        assert_equal(original_balance - cost, new_balance)

