import eth_utils
import rlp
from .client import RpcClient, genesis_addr, ZERO_HASH

import sys
sys.path.append("..")

from conflux.config import default_config
from test_framework.blocktools import create_transaction
from test_framework.util import assert_equal, assert_raises_rpc_error

class TestSendTx(RpcClient):
    def test_encode_invalid_hex(self):
        # empty
        assert_raises_rpc_error(None, None, self.send_raw_tx, "")
        assert_raises_rpc_error(None, None, self.send_raw_tx, "0x")
        # odd length
        assert_raises_rpc_error(None, None, self.send_raw_tx, "0x123")
        # invalid character
        assert_raises_rpc_error(None, None, self.send_raw_tx, "0x123G")

    def _new_test_tx(self, gas_price=1, gas=21000, value=100):
        addr = genesis_addr()
        nonce = self.get_nonce(addr)
        return create_transaction(nonce, gas_price, gas, value)

    def test_encode_invalid_rlp(self):
        tx = self._new_test_tx()
        encoded = eth_utils.encode_hex(rlp.encode(tx))

        assert_raises_rpc_error(None, None, self.send_raw_tx, encoded + "12") # 1 more byte
        assert_raises_rpc_error(None, None, self.send_raw_tx, encoded[0:-2])  # 1 less byte

    def test_signature_empty(self):
        tx = self._new_test_tx()
        # FIXME rpc should return error: tx not signed.
        # assert_raises_rpc_error(None, "None", self.send_tx, tx)
        assert_equal(self.send_tx(tx.copy(r = 0, s = 0, v = 0)), ZERO_HASH)

    def test_signature_invalid(self):
        tx = self._new_test_tx()
        assert_equal(self.send_tx(tx.copy(r = tx.s, s = tx.r)), ZERO_HASH)

     # FIXME remove the prefix "_" to enable this test case.
     # Did not verify the signature when add a tx into pool.
    def _test_signature_data_changed(self):
        tx = self._new_test_tx()
        assert_equal(self.send_tx(tx.copy(value = 333)), ZERO_HASH)

    def test_gas_zero(self):
        tx = self._new_test_tx(gas = 0)
        assert_equal(self.send_tx(tx), ZERO_HASH)

    # FIXME remove the prefix "_" to enable this test case.
    # Intrinsic gas not checked.
    def _test_gas_intrinsic(self):
        tx = self._new_test_tx(gas = 20999)
        assert_equal(self.send_tx(tx), ZERO_HASH)

    def test_gas_too_large(self):
        tx = self._new_test_tx(gas = 10**9 + 1)
        assert_equal(self.send_tx(tx), ZERO_HASH)

    def test_price_zero(self):
        tx = self._new_test_tx(gas_price = 0)
        assert_equal(self.send_tx(tx), ZERO_HASH)

    # FIXME check the maximum size of tx data
    def test_data_too_large(self):
        pass

    # FIXME check the codes (not empty) when create a contract
    def test_data_contract(self):
        pass

    # FIXME return error if account balance is not enough.
    def _test_value_less_than_balance(self):
        addr = genesis_addr()
        balance = self.get_balance(addr)

        tx = self._new_test_tx(value=balance)
        assert_equal(self.send_tx(tx), ZERO_HASH)

    # FIXME return error if account balance is not enough.
    def _test_value_less_than_cost(self):
        addr = genesis_addr()
        balance = self.get_balance(addr)

        # value = balance - gas_fee
        tx = self._new_test_tx(value=balance-21000+1)
        assert_equal(self.send_tx(tx), ZERO_HASH)

    def test_tx_already_processed(self):
        tx = self._new_test_tx()
        tx_hash = self.send_tx(tx, True)
        assert_equal(tx_hash, tx.hash_hex())

        assert_equal(self.send_tx(tx), ZERO_HASH)

    def test_tx_already_pended(self):
        tx = self._new_test_tx()
        tx_hash = self.send_tx(tx)
        assert_equal(tx_hash, tx.hash_hex())

        assert_equal(self.send_tx(tx), ZERO_HASH)
        self.wait_for_receipt(tx_hash, 1)

    # TODO support to replace tx with higher gas price
    def _test_replace_price_too_low(self):
        addr = genesis_addr()
        nonce = self.get_nonce(addr)
        tx = create_transaction(nonce, 10, 21000, 100)
        tx_hash = self.send_tx(tx)
        assert_equal(tx_hash, tx.hash_hex())

        new_tx = create_transaction(nonce, 7, 21000, 100)
        assert_equal(self.send_tx(new_tx), ZERO_HASH)
        self.wait_for_receipt(tx_hash, 1)

    # TODO support to replace tx with higher gas price
    def _test_replace_price(self):
        addr = genesis_addr()
        nonce = self.get_nonce(addr)
        tx = create_transaction(nonce, 10, 21000, 100)
        tx_hash = self.send_tx(tx)
        assert_equal(tx_hash, tx.hash_hex())

        new_tx = create_transaction(nonce, 13, 21000, 100)
        new_tx_hash = self.send_tx(new_tx)
        assert_equal(new_tx_hash, new_tx.hash_hex())
        
        self.wait_for_receipt(new_tx_hash, 1)
