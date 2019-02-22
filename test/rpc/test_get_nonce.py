from .client import RpcClient

import sys
sys.path.append("..")

from test_framework.util import assert_equal, assert_raises_rpc_error

class TestGetNonce(RpcClient):
    def test_account_not_found(self):
        addr = self.rand_addr()
        nonce = self.get_nonce(addr)
        assert_equal(nonce, 0)

    def test_account_exists(self):
        addr = self.GENESIS_ADDR
        nonce = self.get_nonce(addr)

        tx = self.new_tx()
        self.send_tx(tx, True)

        nonce2 = self.get_nonce(addr)
        assert_equal(nonce2, nonce + 1)

    def test_epoch_earliest(self):
        addr = self.GENESIS_ADDR
        nonce = self.get_nonce(addr, self.EPOCH_EARLIEST)
        assert_equal(nonce, 0)

    def test_epoch_latest_state(self):
        addr = self.GENESIS_ADDR
        nonce = self.get_nonce(addr)
        latest_mined_nonce = self.get_nonce(addr, self.EPOCH_LATEST_STATE)
        assert_equal(latest_mined_nonce, nonce)

    # FIXME remove the prefix "_" to enable the test case.
    # Now, the nonce of latest mined is 0. It should raise
    # error instead, please refer to epoch_number("0x6")
    def _test_epoch_latest_mined(self):
        addr = self.GENESIS_ADDR

        nonce = self.get_nonce(addr)
        last_mined_nonce = self.get_nonce(addr, self.EPOCH_LATEST_MINED)
        assert_equal(nonce, last_mined_nonce)

    def test_epoch_num_0(self):
        addr = self.GENESIS_ADDR
        nonce = self.get_nonce(addr, "0x0")
        assert_equal(nonce, 0)

    def test_epoch_num_too_large(self):
        addr = self.GENESIS_ADDR
        epoch = self.epoch_number()
        assert_raises_rpc_error(None, None, self.get_nonce, addr, self.EPOCH_NUM(epoch + 1))

    def test_epoch_num(self):
        addr = self.GENESIS_ADDR

        pre_epoch = self.epoch_number()
        pre_nonce = self.get_nonce(addr)

        # send tx to change the nonce
        tx = self.new_tx(nonce=pre_nonce)
        self.send_tx(tx, True)

        new_nonce = self.get_nonce(addr, self.EPOCH_NUM(pre_epoch))
        assert_equal(new_nonce, pre_nonce)