#!/usr/bin/env python3
import datetime

from eth_utils import decode_hex
from rlp.sedes import Binary, BigEndianInt

from conflux import utils
from conflux.utils import encode_hex, bytes_to_int, privtoaddr, parse_as_int
from test_framework.blocktools import create_block
from test_framework.test_framework import ConfluxTestFramework
# from test_framework.mininode import (
#     P2PInterface,
#     mininode_lock,
#     network_thread_join,
#     network_thread_start,
#     PACKET_HELLO)
from test_framework.mininode import *
from test_framework.util import *


class P2PTest(ConfluxTestFramework):
    def set_test_params(self):
        self.setup_clean_chain = True
        self.num_nodes = 16

    def setup_network(self):
        self.add_nodes(self.num_nodes)
        bootnode = self.nodes[0]
        extra_args0 = ["--enable-discovery", "true", "--node-table-timeout", "1", "--node-table-promotion-timeout", "1"]
        self.start_node(0, extra_args = extra_args0)
        bootnode_id = "cfxnode://{}@{}:{}".format(bootnode.key[2:], bootnode.ip, bootnode.port)
        self.node_extra_args = ["--bootnodes", bootnode_id, "--enable-discovery", "true", "--node-table-timeout", "1", "--node-table-promotion-timeout", "1"]
        for i in range(1, self.num_nodes):
            self.start_node(i, extra_args=self.node_extra_args)
        for i in range(self.num_nodes):
            wait_until(lambda: len(self.nodes[i].getpeerinfo()) >= 2)

    def run_test(self):
        block_number = 10

        for i in range(1, block_number):
            chosen_peer = random.randint(0, self.num_nodes - 1)
            block_hash = self.nodes[chosen_peer].generate(1, 10)
            self.log.info("generate block %s", block_hash)
        wait_for_block_count(self.nodes[0], block_number, timeout=30)
        sync_blocks(self.nodes, timeout=30)
        self.log.info("generated blocks received by all")

        self.stop_node(0, kill=True)
        self.log.info("node 0 stopped")
        block_hash = self.nodes[-1].generate(1, 10)
        self.log.info("generate block %s", block_hash)
        wait_for_block_count(self.nodes[1], block_number + 1)
        sync_blocks(self.nodes[1:], timeout=30)
        self.log.info("blocks sync success among running nodes")
        self.start_node(0)
        sync_blocks(self.nodes, timeout=30)
        self.log.info("Pass")


class DefaultNode(P2PInterface):
    def __init__(self):
        super().__init__()
        self.protocol = b'cfx'
        self.protocol_version = 1


if __name__ == "__main__":
    P2PTest().main()
