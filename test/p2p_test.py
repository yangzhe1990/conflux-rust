#!/usr/bin/env python3
from rlp.sedes import Binary, BigEndianInt

from conflux import utils
from conflux.utils import encode_hex, bytes_to_int
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
        self.num_nodes = 4
        self.block_number = 10
        self.difficulty = 10
        self.generate_period = 2

    def setup_network(self):
        self.setup_nodes()
        for i in range(self.num_nodes - 1):
            for j in range(i+1, self.num_nodes):
                connect_nodes(self.nodes, i, j)

    def run_test(self):
        default_node = self.nodes[0].add_p2p_connection(DefaultNode())
        network_thread_start()
        self.nodes[0].p2p.wait_for_status()

        """Start Some P2P Message Test From Here"""
        blocks = [default_node.genesis.block_header.hash]
        tip = default_node.genesis.block_header.hash
        block_time = 1

        for i in range(1, self.block_number):
            # Use the mininode and blocktools functionality to manually build a block
            # Calling the generate() rpc is easier, but this allows us to exactly
            # control the blocks and transactions.
            block = create_block(tip, block_time, self.difficulty)
            self.nodes[0].p2p.send_protocol_msg(NewBlock(block=block))
            tip = block.block_header.hash
            blocks.append(block)
            block_time += 1
        wait_for_block_count(self.nodes[0], self.block_number)
        sync_blocks(self.nodes)
        # self.nodes[0].generate(1)
        # wait_for_block_count(self.nodes[0], self.block_number+1)
        print("pass")


class DefaultNode(P2PInterface):
    def __init__(self):
        super().__init__()
        self.protocol = b'cfx'
        self.protocol_version = 1


if __name__ == "__main__":
    P2PTest().main()
