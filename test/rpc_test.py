from test_framework.test_framework import ConfluxTestFramework
from test_framework.util import assert_equal, connect_nodes, get_peer_addr, wait_until


class RpcTest(ConfluxTestFramework):
    def set_test_params(self):
        self.setup_clean_chain = True
        self.num_nodes = 2
        self.best_block_hash = None
        self.block_bumber = 10

    def setup_network(self):
        self.setup_nodes()

    def run_test(self):
        blocks = self.nodes[0].generate(self.block_bumber, 0)
        self.best_block_hash = blocks[-1]

        self._test_getblockcount()
        self._test_sayhello()
        self._test_getbalance()
        self._test_getbestblockhash()
        self._test_getblock()
        self._test_getpeerinfo()

        # Test stop at last
        self._test_stop()

    def _test_sayhello(self):
        self.log.info("Test sayhello")
        hello_string = "Hello, world"
        res = self.nodes[0].sayhello()
        assert_equal(hello_string, res)

    def _test_getblockcount(self):
        self.log.info("Test getblockcount")
        res = self.nodes[0].getblockcount()
        assert_equal(self.block_bumber, res)

    def _test_getbalance(self):
        self.log.info("Test getbalance")
        addr = "0x" + "0" * 40
        res = self.nodes[0].getbalance(addr)
        balance = int(res, 0)
        assert_equal(10 ** 9, balance)

    def _test_getbestblockhash(self):
        self.log.info("Test getbestblockhash")
        res = self.nodes[0].getbestblockhash()
        assert_equal(self.best_block_hash, res)

    def _test_getblock(self):
        self.log.info("Test getbestblockhash")
        res = self.nodes[0].getblock(self.best_block_hash)
        assert_equal(self.best_block_hash, res['hash'])

    def _test_getpeerinfo(self):
        self.log.info("Test getpeerinfo")
        connect_nodes(self.nodes[0], 1, self.nodes[1].key)
        res = self.nodes[0].getpeerinfo()
        assert_equal(len(res), 1)
        assert_equal(res[0]['addr'], get_peer_addr(1))
        self.nodes[0].removenode(self.nodes[1].key, get_peer_addr(1))
        try:
            wait_until(lambda: len(self.nodes[0].getpeerinfo()) == 0, timeout=10)
        except Exception:
            assert False

    def _test_stop(self):
        self.log.info("Test stop")
        try:
            self.nodes[0].stop()
            self.nodes[0].getpeerinfo()
            assert False
        except Exception:
            pass


if __name__ == "__main__":
    RpcTest().main()
