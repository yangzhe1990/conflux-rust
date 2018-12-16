#!/usr/bin/env python3
from eth_utils import decode_hex, encode_hex

from test_framework.blocktools import create_block, create_transaction
from test_framework.test_framework import ConfluxTestFramework
from test_framework.mininode import *
from test_framework.util import *


class P2PTest(ConfluxTestFramework):
    def set_test_params(self):
        self.setup_clean_chain = True
        self.num_nodes = 5

    def setup_network(self):
        self.add_nodes(self.num_nodes)
        for i in range(self.num_nodes):
            self.start_node(i)
        for i in range(1, self.num_nodes - 1):
            connect_nodes(self.nodes, i, i+1)

    def run_test(self):
        block_number = 100

        self.nodes[0].add_p2p_connection(DefaultNode())
        self.nodes[1].add_p2p_connection(DefaultNode())
        network_thread_start()
        self.nodes[0].p2p.wait_for_status()
        self.nodes[1].p2p.wait_for_status()
        best_block = self.nodes[1].generate(1, 10)[0]
        block1 = create_block(parent_hash=decode_hex(best_block), height=2)
        block2 = create_block(parent_hash=decode_hex(best_block), height=2, transactions=[create_transaction()])
        self.nodes[1].p2p.send_protocol_msg(NewBlock(block=block1))
        self.nodes[1].p2p.send_protocol_msg(NewBlock(block=block2))
        block3 = create_block(parent_hash=block1.hash, height=3, referee_hashes=[block2.hash])
        self.nodes[1].p2p.send_protocol_msg(NewBlock(block=block3))
        connect_nodes(self.nodes, 0, 1)
        sync_blocks(self.nodes, timeout=5)
        best_block = self.nodes[0].getbestblockhash()
        assert_equal(best_block, encode_hex(block3.hash))
        self.log.info("Pass 1")
        for block in [block1, block2, block3]:
            print(encode_hex(block.hash))

        disconnect_nodes(self.nodes, 0, 1)
        block1 = create_block(parent_hash=decode_hex(best_block), height=4)
        block2 = create_block(parent_hash=decode_hex(best_block), height=4, transactions=[create_transaction()])
        self.nodes[0].p2p.send_protocol_msg(NewBlock(block=block1))
        self.nodes[1].p2p.send_protocol_msg(NewBlock(block=block2))
        connect_nodes(self.nodes, 0, 1)
        sync_blocks(self.nodes, timeout=5)
        self.log.info("Pass 2")

        disconnect_nodes(self.nodes, 0, 1)
        for i in range(block_number):
            chosen_peer = random.randint(1, self.num_nodes - 1)
            block_hash = self.nodes[chosen_peer].generate(1, 10)
            self.log.info("%s generate block %s", chosen_peer, block_hash)
        wait_for_block_count(self.nodes[1], block_number + 7)
        sync_blocks(self.nodes[1:], timeout=10)
        self.log.info("blocks sync successfully between old nodes")
        connect_nodes(self.nodes, 0, 1)
        sync_blocks(self.nodes, timeout=30)
        self.log.info("Pass 3")


class DefaultNode(P2PInterface):
    def __init__(self):
        super().__init__()
        self.protocol = b'cfx'
        self.protocol_version = 1


if __name__ == "__main__":
    P2PTest().main()
