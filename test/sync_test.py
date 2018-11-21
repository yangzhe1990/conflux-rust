#!/usr/bin/env python3
from test_framework.test_framework import ConfluxTestFramework
from test_framework.mininode import *
from test_framework.util import *


class P2PTest(ConfluxTestFramework):
    def set_test_params(self):
        self.setup_clean_chain = True
        self.num_nodes = 5

    def setup_network(self):
        self.add_nodes(self.num_nodes)
        for i in range(self.num_nodes - 1):
            self.start_node(i)
        for i in range(self.num_nodes - 2):
            connect_nodes(self.nodes, i, i+1)

    def run_test(self):
        block_number = 50

        for i in range(1, block_number):
            chosen_peer = random.randint(0, self.num_nodes - 2)
            block_hash = self.nodes[chosen_peer].generate(1, 10)
            self.log.info("generate block %s", block_hash)
        wait_for_block_count(self.nodes[0], block_number)
        sync_blocks(self.nodes[:2], timeout=10)
        self.log.info("blocks sync successfully between old nodes")
        self.start_node(self.num_nodes - 1)
        connect_nodes(self.nodes, self.num_nodes - 1, 0)
        sync_blocks(self.nodes, timeout=30)
        self.log.info("Pass")


class DefaultNode(P2PInterface):
    def __init__(self):
        super().__init__()
        self.protocol = b'cfx'
        self.protocol_version = 1


if __name__ == "__main__":
    P2PTest().main()
