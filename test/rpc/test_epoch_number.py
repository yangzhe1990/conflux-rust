from .client import RpcClient

import sys
sys.path.append("..")

from test_framework.util import assert_equal, assert_raises_rpc_error

class TestEpochNumber(RpcClient):
    def test_num_default(self):
        epoch = self.epoch_number()
        
        # 1 new blocks
        self.generate_block(1)
        epoch2 = self.epoch_number()
        assert_equal(epoch + 1, epoch2)

        # N new blocks
        self.generate_blocks(13, 1)
        epoch3 = self.epoch_number()
        assert_equal(epoch2 + 13, epoch3)

    def test_num_valid(self):
        self.epoch_number("latest_mined")
        self.epoch_number("latest_state")
        
        assert_equal(self.epoch_number("earliest"), 0)

    def test_num_invalid(self):
        assert_raises_rpc_error(None, None, self.epoch_number, "")
        assert_raises_rpc_error(None, None, self.epoch_number, "dummy_num")
        assert_raises_rpc_error(None, None, self.epoch_number, "latest_mined".upper())
        assert_raises_rpc_error(None, None, self.epoch_number, "Latest_mined")
        assert_raises_rpc_error(None, None, self.epoch_number, "6")
        assert_raises_rpc_error(None, None, self.epoch_number, "0X5")
        assert_raises_rpc_error(None, None, self.epoch_number, "0xg")

        # EpochNumber::Num(hex) is not supported
        self.generate_blocks(3, 1)
        assert_raises_rpc_error(None, None, self.epoch_number, "0x3")