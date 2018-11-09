#!/usr/bin/env python3
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
        print("genesis", encode_hex(default_node.genesis.block_header.hash))
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
            # block = create_block(tip, block_time, self.difficulty)
            # self.nodes[0].p2p.send_protocol_msg(NewBlock(block=block))
            # tip = block.block_header.hash
            # blocks.append(block)
            # block_time += 1
            # print("generate block", encode_hex(tip))
            self.nodes[0].generate(1, 10)
        wait_for_block_count(self.nodes[0], self.block_number)
        sync_blocks(self.nodes)
        print(self.nodes[0].getbestblockhash())
        balance = self.nodes[0].getbalance(eth_utils.encode_hex(privtoaddr(decode_hex("0x46b9e861b63d3509c88b7817275a30d22d62c8cd8fa6486ddee35ef0d8e0495f"))))
        print(parse_as_int(balance))
        # assert_equal(self.nodes[0].getbestblockhash()[2:], encode_hex(tip))
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
