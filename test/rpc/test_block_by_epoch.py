from .client import RpcClient

import sys
sys.path.append("..")

from test_framework.util import assert_equal, assert_raises_rpc_error

class TestGetBlockByEpoch(RpcClient):
    def test_last_mined(self):
        block_hash = self.generate_block()
        block = self.block_by_epoch(self.EPOCH_LATEST_MINED)
        assert_equal(block["hash"], block_hash)

    def test_earliest(self):
        block = self.block_by_epoch(self.EPOCH_EARLIEST)
        assert_equal(int(block["epochNumber"], 0), 0)

    def test_epoch_num(self):
        block_hash = self.generate_block()
        block = self.block_by_hash(block_hash)
        epoch_num = block["epochNumber"]

        block = self.block_by_epoch(epoch_num)
        assert_equal(block["hash"], block_hash)

    def test_epoch_not_found(self):
        block_hash = self.generate_block()
        block = self.block_by_hash(block_hash)
        epoch_num = block["epochNumber"]

        large_epoch = int(epoch_num, 0) + 1
        assert_raises_rpc_error(None, None, self.block_by_epoch, self.EPOCH_NUM(large_epoch), False)