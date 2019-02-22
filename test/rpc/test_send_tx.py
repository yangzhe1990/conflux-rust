import eth_utils
import rlp
from .client import RpcClient, DEFAULT_TX_FEE

import sys
sys.path.append("..")

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

    def test_encode_invalid_rlp(self):
        tx = self.new_tx()
        encoded = eth_utils.encode_hex(rlp.encode(tx))

        assert_raises_rpc_error(None, None, self.send_raw_tx, encoded + "12") # 1 more byte
        assert_raises_rpc_error(None, None, self.send_raw_tx, encoded[0:-2])  # 1 less byte

    def test_signature_empty(self):
        tx = self.new_tx()
        # FIXME rpc should return error: tx not signed.
        # assert_raises_rpc_error(None, "None", self.send_tx, tx)
        assert_equal(self.send_tx(tx.copy(r = 0, s = 0, v = 0)), self.ZERO_HASH)

    def test_signature_invalid(self):
        tx = self.new_tx()
        assert_equal(self.send_tx(tx.copy(r = tx.s, s = tx.r)), self.ZERO_HASH)

     # FIXME remove the prefix "_" to enable this test case.
     # Did not verify the signature when add a tx into pool.
    def _test_signature_data_changed(self):
        tx = self.new_tx()
        assert_equal(self.send_tx(tx.copy(value = 333)), self.ZERO_HASH)

    def test_gas_zero(self):
        tx = self.new_tx(gas = 0)
        assert_equal(self.send_tx(tx), self.ZERO_HASH)

    # FIXME remove the prefix "_" to enable this test case.
    # Intrinsic gas not checked.
    def _test_gas_intrinsic(self):
        tx = self.new_tx(gas = 20999)
        assert_equal(self.send_tx(tx), self.ZERO_HASH)

    def test_gas_too_large(self):
        tx = self.new_tx(gas = 10**9 + 1)
        assert_equal(self.send_tx(tx), self.ZERO_HASH)

    def test_price_zero(self):
        tx = self.new_tx(gas_price = 0)
        assert_equal(self.send_tx(tx), self.ZERO_HASH)

    # FIXME check the maximum size of tx data
    def test_data_too_large(self):
        pass

    # FIXME check the codes (not empty) when create a contract
    def test_data_contract(self):
        pass

    # FIXME return error if account balance is not enough.
    def _test_value_less_than_balance(self):
        balance = self.get_balance(self.GENESIS_ADDR)

        tx = self.new_tx(value=balance)
        assert_equal(self.send_tx(tx), self.ZERO_HASH)

    # FIXME return error if account balance is not enough.
    def _test_value_less_than_cost(self):
        balance = self.get_balance(self.GENESIS_ADDR)

        # value = balance - gas_fee
        tx = self.new_tx(value=balance-DEFAULT_TX_FEE+1)
        assert_equal(self.send_tx(tx), self.ZERO_HASH)

    def test_tx_already_processed(self):
        tx = self.new_tx()
        tx_hash = self.send_tx(tx, True)
        assert_equal(tx_hash, tx.hash_hex())

        assert_equal(self.send_tx(tx), self.ZERO_HASH)

    def test_tx_already_pended(self):
        tx = self.new_tx()
        tx_hash = self.send_tx(tx)
        assert_equal(tx_hash, tx.hash_hex())

        assert_equal(self.send_tx(tx), self.ZERO_HASH)
        self.wait_for_receipt(tx_hash)

    # TODO support to replace tx with higher gas price
    def _test_replace_price_too_low(self):
        cur_nonce = self.get_nonce(self.GENESIS_ADDR)
        tx = self.new_tx(nonce=cur_nonce, gas_price=10)
        tx_hash = self.send_tx(tx)
        assert_equal(tx_hash, tx.hash_hex())

        new_tx = self.new_tx(nonce=cur_nonce, gas_price=7)
        assert_equal(self.send_tx(new_tx), self.ZERO_HASH)
        self.wait_for_receipt(tx_hash)

    # TODO support to replace tx with higher gas price
    def _test_replace_price(self):
        cur_nonce = self.get_nonce(self.GENESIS_ADDR)
        tx = self.new_tx(nonce=self.new_tx, gas_price=10)
        tx_hash = self.send_tx(tx)
        assert_equal(tx_hash, tx.hash_hex())

        new_tx = self.new_tx(nonce=cur_nonce, gas_price=13)
        new_tx_hash = self.send_tx(new_tx)
        assert_equal(new_tx_hash, new_tx.hash_hex())
        
        self.wait_for_receipt(new_tx_hash)

    def test_larger_nonce(self):
        # send tx with nonce + 1
        cur_nonce = self.get_nonce(self.GENESIS_ADDR)
        tx = self.new_tx(nonce=cur_nonce+1)
        tx_hash = self.send_tx(tx)
        assert_equal(tx_hash, tx.hash_hex())

        # tx with nonce + 1 should be in pool even after N blocks mined
        self.generate_blocks_to_state()
        assert_equal(self.get_tx(tx_hash), None)

        # send another tx with nonce and stated with receipt
        tx3 = self.new_tx(nonce=cur_nonce)
        assert_equal(self.send_tx(tx3, True), tx3.hash_hex())

        # tx with nonce + 1 is ok for state
        self.wait_for_receipt(tx_hash)
        assert_equal(self.get_tx(tx_hash)["hash"], tx_hash)
