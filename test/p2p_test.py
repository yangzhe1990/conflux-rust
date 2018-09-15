#!/usr/bin/env python3

from test_framework.test_framework import ConfluxTestFramework
from test_framework.mininode import (
    P2PInterface,
    mininode_lock,
    network_thread_join,
    network_thread_start,
)
from test_framework.util import *


class P2PTest(ConfluxTestFramework):
    def set_test_params(self):
        self.setup_clean_chain = True
        self.num_nodes = 1

    def setup_network(self):
        self.setup_nodes()

    def run_test(self):
        self.nodes[0].add_p2p_connection(P2PInterface())

        network_thread_start()

        while True:
            pass


if __name__ == "__main__":
    P2PTest().main()
