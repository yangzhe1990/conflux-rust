#!/usr/bin/env python3
from rlp.sedes import Binary, BigEndianInt

from conflux import utils
from conflux.utils import encode_hex, bytes_to_int
from test_framework.blocktools import create_block, create_transaction
from test_framework.test_framework import ConfluxTestFramework
# from test_framework.mininode import (
#     P2PInterface,
#     mininode_lock,
#     network_thread_join,
#     network_thread_start,
#     PACKET_HELLO)
from test_framework.mininode import *
from test_framework.util import *


class MessageTest(ConfluxTestFramework):
    def set_test_params(self):
        self.setup_clean_chain = True
        self.num_nodes = 4

    def setup_network(self):
        self.setup_nodes()
        for i in range(self.num_nodes - 1):
            connect_nodes(self.nodes, i, i + 1)

    def run_test(self):
        default_node = self.nodes[0].add_p2p_connection(DefaultNode())
        network_thread_start()
        self.nodes[0].p2p.wait_for_status()

        """Start Some P2P Message Test From Here"""
        blocks = [default_node.genesis.block_header.hash]
        tip = default_node.genesis.block_header.hash
        # b = self.nodes[0].getblock(self.nodes[0].getbestblockhash())
        # block_time = int(b['timestamp'], 0) + 1
        block_time = int(1)

        # Use the mininode and blocktools functionality to manually build a block
        # Calling the generate() rpc is easier, but this allows us to exactly
        # control the blocks and transactions.
        new_block = create_block(tip, block_time)
        new_transaction = create_transaction()

        self.send_msg(GetBlockHashes(hash=blocks[0], max_blocks=1))
        self.send_msg(GetBlockHeaders(hash=blocks[0], max_blocks=1))
        self.send_msg(GetBlockBodies(hashes=[blocks[0]]))
        self.send_msg(GetBlocks(hashes=[blocks[0]]))
        self.send_msg(BlockHashes(hashes=[blocks[0]]))
        self.send_msg(BlockHeaders(headers=[default_node.genesis.block_header]))
        self.send_msg(BlockBodies(bodies=[default_node.genesis]))
        self.send_msg(Blocks(blocks=[default_node.genesis]))
        self.send_msg(NewBlockHashes(block_hashes=[BlockHash(number=1, hash=new_block.block_header.hash)]))
        self.send_msg(NewBlock(block=new_block))
        self.send_msg(GetTerminalBlockHashes())
        self.send_msg(TerminalBlockHashes(hashes=[blocks[0]]))
        self.send_msg(Transactions(transactions=[new_transaction]))

        # wait_for_block_count(self.nodes[0], self.block_number)
        # sync_blocks(self.nodes)
        # self.nodes[0].generate(1)
        # wait_for_block_count(self.nodes[0], self.block_number+1)
        print("pass")
        while True:
            pass

    def send_msg(self, msg):
        self.nodes[0].p2p.send_protocol_msg(msg)


class DefaultNode(P2PInterface):
    def __init__(self):
        super().__init__()
        self.protocol = b'cfx'
        self.protocol_version = 1


if __name__ == "__main__":
    MessageTest().main()
