from .client import RpcClient, genesis_addr

import sys
sys.path.append("..")

from test_framework.blocktools import create_transaction
from test_framework.util import assert_equal, assert_greater_than

class TestGasPrice(RpcClient):
    # FIXME remove the "_" prefix to enable this test case
    # once default gas price defined.
    def _test_default_value(self):
        price = self.gas_price()
        assert_greater_than(price, 0)

    def test_nochange_without_tx(self):
        price = self.gas_price()
        self.generate_block(1)
        price2 = self.gas_price()
        assert_equal(price, price2)

    def test_median_prices(self):
        sender = genesis_addr()

        prices = [7,5,1,9,3]
        txs = []
        nonce = self.get_nonce(sender)

        # generate 100 blocks to make sure the gas price is only decided by below txs.
        self.generate_blocks(100, 1)

        # sent txs
        for p in prices:
            tx = create_transaction(nonce, gas_price=p, value=100)
            tx_hash = self.send_tx(tx)
            txs.append(tx_hash)
            nonce += 1

        # generate 5 blocks to pack the sent txs
        self.generate_blocks(5, len(txs))

        # wait for receipts of sent txs
        for tx in txs:
            self.wait_for_receipt(tx, len(txs), 10)

        # median of prices
        assert_equal(self.gas_price(), 5)

