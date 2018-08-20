#!/usr/bin/env python3
"""An example functional test
"""

from test_framework.test_framework import ConfluxTestFramework
from test_framework.util import *


class ExampleTest(ConfluxTestFramework):
    def set_test_params(self):
        self.setup_clean_chain = True
        self.num_nodes = 2

    def setup_network(self):
        self.setup_nodes()

    def run_test(self):
        genesis = self.nodes[0].getbestblockhash()
        self.log.info(genesis)

        self.nodes[0].generate(1)
        assert (self.nodes[0].getblockcount() == 1)
        besthash = self.nodes[0].getbestblockhash()

        self.nodes[1].generate(2)
        print(self.nodes[1].getblockcount())
        assert (self.nodes[1].getblockcount() == 2)

        connect_nodes_bi(self.nodes, 0, 1)
        sync_blocks(self.nodes[0:2])
        assert (self.nodes[0].getblockcount() == 2)


if __name__ == '__main__':
    ExampleTest().main()