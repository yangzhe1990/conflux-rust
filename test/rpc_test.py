import datetime
import time

import eth_utils

from conflux.messages import GetBlockHeaders
from conflux.utils import int_to_hex
from test_framework.blocktools import make_genesis
from test_framework.mininode import network_thread_start, P2PInterface
from test_framework.test_framework import ConfluxTestFramework
from test_framework.util import assert_equal, connect_nodes, get_peer_addr, wait_until


class RpcTest(ConfluxTestFramework):
    def set_test_params(self):
        self.setup_clean_chain = True
        self.num_nodes = 2

    def setup_network(self):
        self.setup_nodes()

    def run_test(self):
        # blocks = self.nodes[0].generate(self.block_bumber, 0)
        self.best_block_hash = make_genesis(10).block_header.hash
        self.block_number = 1

        self._test_getblockcount()
        self._test_sayhello()
        self._test_getbalance()
        self._test_getbestblockhash()
        self._test_getblock()
        self._test_getpeerinfo()
        self._test_addlatency()

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
        assert_equal(self.block_number, res)

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
        res = self.nodes[0].getblock(eth_utils.encode_hex(self.best_block_hash))
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

    def _test_addlatency(self):
        class DefaultNode(P2PInterface):
            def on_block_headers(self, headers):
                assert (datetime.datetime.now() - self.start_time).total_seconds() * 1000 >= self.latency_ms
                self.wait = False

        default_node = self.nodes[0].add_p2p_connection(DefaultNode())
        network_thread_start()
        self.nodes[0].p2p.wait_for_status()
        latency_ms = 1000
        x, y = default_node.pub_key
        self.nodes[0].addlatency("0x"+int_to_hex(x)[2:]+int_to_hex(y)[2:], latency_ms)
        default_node.start_time = datetime.datetime.now()
        default_node.latency_ms = latency_ms
        default_node.wait = True
        self.nodes[0].p2p.send_protocol_msg(GetBlockHeaders(hash=default_node.genesis.block_header.hash, max_blocks=1))
        wait_until(lambda: not default_node.wait)

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
