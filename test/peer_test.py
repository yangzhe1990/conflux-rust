#!/usr/bin/env python3
from rlp.sedes import Binary, BigEndianInt

from conflux import utils
from conflux.utils import encode_hex, bytes_to_int, get_nodeid
from test_framework.blocktools import create_block
from test_framework.test_framework import ConfluxTestFramework
# from test_framework.mininode import (
#     P2PInterface,
#     mininode_lock,
#     network_thread_join,
#     network_thread_start,
#     PACKET_HELLO)
from test_framework.mininode import *
from test_framework.test_node import TestNode
from test_framework.util import *


class PeerTest(ConfluxTestFramework):
    def set_test_params(self):
        self.setup_clean_chain = True
        self.num_nodes = 4

    def setup_network(self):
        self.add_nodes(self.num_nodes)
        bootnode = self.nodes[0]
        self.start_node(0)
        bootnode_id = "cfxnode://{}@{}:{}".format(bootnode.key, bootnode.ip, bootnode.port)
        extra_args = ["--bootnodes", bootnode_id]
        for i in range(1, self.num_nodes):
            self.start_node(i, extra_args=extra_args)

    def run_test(self):
        wait_until(lambda: [len(i.getpeerinfo()) for i in self.nodes].count(self.num_nodes - 1) == self.num_nodes)
        print("pass")


class DefaultNode(P2PInterface):
    def __init__(self):
        super().__init__()
        self.protocol = b'cfx'
        self.protocol_version = 1


if __name__ == "__main__":
    PeerTest().main()
