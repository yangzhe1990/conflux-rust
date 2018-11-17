#!/usr/bin/env python3
import datetime
import time

import eth_utils

from conflux.messages import GetBlockHeaders
from conflux.utils import int_to_hex, privtoaddr
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
        self.block_number = 10
        blocks = self.nodes[0].generate(self.block_number, 0)
        self.best_block_hash = blocks[-1] #make_genesis().block_header.hash

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
        assert_equal(self.block_number + 1, res)

    def _test_getbalance(self):
        self.log.info("Test getbalance")
        addr = eth_utils.encode_hex(privtoaddr(eth_utils.decode_hex("46b9e861b63d3509c88b7817275a30d22d62c8cd8fa6486ddee35ef0d8e0495f")))
        res = self.nodes[0].getbalance(addr)
        balance = int(res, 0)
        assert_equal(10 ** 9, balance)

    def _test_getbestblockhash(self):
        self.log.info("Test getbestblockhash")
        res = self.nodes[0].getbestblockhash()
        assert_equal(self.best_block_hash, res)

    def _test_getblock(self):
        self.log.info("Test getblock")
        res = self.nodes[0].getblock(self.best_block_hash)
        self.log.info(res)
        assert_equal(self.best_block_hash, res['hash'])
        assert_equal(self.block_number, res['number'])

    def _test_getpeerinfo(self):
        self.log.info("Test getpeerinfo")
        connect_nodes(self.nodes, 0, 1)
        res = self.nodes[0].getpeerinfo()
        assert_equal(len(res), 1)
        assert_equal(res[0]['addr'], get_peer_addr(self.nodes[0], 1))
        self.nodes[0].removenode(self.nodes[1].key, get_peer_addr(self.nodes[0], 1))
        try:
            wait_until(lambda: len(self.nodes[0].getpeerinfo()) == 0, timeout=10)
        except Exception:
            assert False

    def _test_addlatency(self):
        class DefaultNode(P2PInterface):
            def __init__(self, test):
                super().__init__()
                self.test = test
            def on_block_headers(self, headers):
                msec = (datetime.datetime.now() - self.start_time).total_seconds() * 1000
                self.test.log.info("Message arrived after " + str(msec) + "ms")
                # The EventLoop in rust may have a deviation of a maximum of
                # 100ms. This is because the ticker is 100ms by default.
                assert msec >= self.latency_ms - 100
                self.wait = False
        
        self.log.info("Test addlatency")
        default_node = self.nodes[0].add_p2p_connection(DefaultNode(self))
        network_thread_start()
        self.nodes[0].p2p.wait_for_status()
        latency_ms = 1000
        self.nodes[0].addlatency(default_node.key, latency_ms)
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
