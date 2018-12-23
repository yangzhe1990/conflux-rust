#!/usr/bin/env python3
from rlp.sedes import Binary, BigEndianInt

from conflux import utils
from conflux.utils import encode_hex, bytes_to_int, convert_to_nodeid
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
        self.num_nodes = 0

    def setup_network(self):
        self.setup_nodes()
        # for i in range(self.num_nodes - 1):
        #     connect_nodes_bi(self.nodes, i, i + 1)

    def run_test(self):

        # Start mininode connection
        default_node = DefaultNode()
        self.node = default_node
        kwargs = {}
        args = {}
        kwargs['dstport'] = 32323
        kwargs['dstaddr'] = '127.0.0.1'
        default_node.peer_connect(*args, **kwargs)
        network_thread_start()
        default_node.wait_for_status()

        # Start rpc connection
        rpc = get_rpc_proxy(
            "http://127.0.0.1:11000",
            1,
            timeout=10)
        challenge = random.randint(0, 2**32-1)
        signature = rpc.getnodeid(list(int_to_bytes(challenge)))
        node_id = convert_to_nodeid(signature, challenge)
        print("get nodeid", eth_utils.encode_hex(node_id))
        # b = self.nodes[0].getblock(self.nodes[0].getbestblockhash())
        # block_time = int(b['timestamp'], 0) + 1

        blocks = [default_node.genesis.block_header.hash]
        tip = default_node.genesis.block_header.hash
        # b = self.nodes[0].getblock(self.nodes[0].getbestblockhash())
        # block_time = int(b['timestamp'], 0) + 1
        block_time = int(1)
        block_number = 10

        for i in range(1, block_number):
            # Use the mininode and blocktools functionality to manually build a block
            # Calling the generate() rpc is easier, but this allows us to exactly
            # control the blocks and transactions.

            block = create_block(tip, block_time)
            self.node.send_protocol_msg(NewBlock(block=block))
            tip = block.block_header.hash
            blocks.append(block)
            block_time += 1

        wait_for_block_count(rpc, block_number)
        rpc.generate(1, 0)
        wait_for_block_count(rpc, block_number+1)
        print("pass")
        while True:
            pass

    def send_msg(self, msg):
        self.node.send_protocol_msg(msg)


class DefaultNode(P2PInterface):
    def __init__(self):
        super().__init__()
        self.protocol = b'cfx'
        self.protocol_version = 1


if __name__ == "__main__":
    MessageTest().main()
