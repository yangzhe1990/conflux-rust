#!/usr/bin/env python3
"""An example functional test
"""

from test_framework.test_framework import ConfluxTestFramework

# from test_framework.util import (assert_equal, connect_nodes, wait_util)


class ExampleTest(ConfluxTestFramework):
    def set_test_params(self):
        self.setup_clean_chain = True
        self.num_nodes = 3

    def setup_network(self):
        """Setup the test network topology

        Often you won't need to override this, since the standard network topology
        (linear: node0 <-> node1 <-> node2 <-> ...) is fine for most tests.

        If you do override this method, remember to start the nodes, assign
        them to self.nodes, connect them and then sync."""

        self.setup_nodes()

        # In this test, we're not connecting node2 to node0 or node1. Calls to
        # sync_all() should not include node2, since we're not expecting it to
        # sync.
        connect_nodes(self.nodes[0], 1)
        self.sync_all([self.nodes[0:1]])

    def run_test(self):
        # Generating a block on one of the nodes will get us out of IBD
        blocks = [int(self.nodes[0].generate(nblocks=1)[0], 16)]
        self.sync_all([self.nodes[0:1]])

        # Notice above how we called an RPC by calling a method with the same
        # name on the node object. Notice also how we used a keyword argument
        # to specify a named RPC argument. Neither of those are defined on the
        # node object. Instead there's some __getattr__() magic going on under
        # the covers to dispatch unrecognised attribute calls to the RPC
        # interface.

        # Logs are nice. Do plenty of them. They can be used in place of comments for
        # breaking the test into sub-sections.
        self.log.info("Starting test!")

        self.log.info("Create some blocks")
        self.tip = int(self.nodes[0].getbestblockhash(), 16)
        # self.block_time = self.nodes[0].getblock(
        #     self.nodes[0].getbestblockhash())['time'] + 1


if __name__ == '__main__':
    ExampleTest().main()