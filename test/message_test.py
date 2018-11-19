#!/usr/bin/env python3
from rlp.sedes import Binary, BigEndianInt

from conflux import utils
from conflux.utils import encode_hex, bytes_to_int, int_to_hex
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
import time

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

        # Use the mininode and blocktools functionality to manually build a block
        # Calling the generate() rpc is easier, but this allows us to exactly
        # control the blocks and transactions.
        blocks = [default_node.genesis.block_header.hash]
        new_block = create_block(blocks[0], 1)
        new_transaction = create_transaction()

        # This message is not used in current Conflux sync protocol
        # self.log.info("Send GetBlockHashes message")
        # self.send_msg(GetBlockHashes(hash=blocks[0], max_blocks=1))
        # wait_until(lambda: default_node.msg_count >= 1)
        def on_block_headers(node, msg):
            self.log.info("Received %d headers", len(msg.headers))
            for header in msg.headers:
                self.log.info("Block header: %s", encode_hex(header.hash))
        handler = WaitHandler(default_node, GET_BLOCK_HEADERS_RESPONSE, on_block_headers)
        self.log.info("Send GetBlockHeaders message")
        self.send_msg(GetBlockHeaders(hash=blocks[0], max_blocks=1))
        handler.wait()
        # This message is not used in current Conflux sync protocol
        # self.log.info("Send GetBlockBoies message")
        # self.send_msg(GetBlockBodies(hashes=[blocks[0]]))
        # wait_until(lambda: default_node.msg_count >= 3)
        self.log.info("Send GetBlocks message")
        handler = WaitHandler(default_node, GET_BLOCKS_RESPONSE)
        self.send_msg(GetBlocks(hashes=[blocks[0]]))
        handler.wait()
        self.log.info("Received GetBlock response")

        self.send_msg(NewBlockHashes([new_block.block_header.hash]))
        self.send_msg(NewBlock(block=new_block))
        self.log.info("Send GetTerminalBLockHashes message")
        self.send_msg(GetTerminalBlockHashes())
        handler = WaitHandler(default_node, GET_TERMINAL_BLOCK_HASHES_RESPONSE)
        handler.wait()
        self.log.info("Received TerminalBlockHashes")

        self.send_msg(Transactions(transactions=[new_transaction]))
        time.sleep(5)
        res = self.nodes[0].getstatus()
        assert_equal(1, res['pendingTxNumber'])
        res = self.nodes[1].getstatus()
        assert_equal(1, res['pendingTxNumber'])
        self.log.info("Pass")

    def send_msg(self, msg):
        self.nodes[0].p2p.send_protocol_msg(msg)


class DefaultNode(P2PInterface):
    def __init__(self):
        super().__init__()
        self.protocol = b'cfx'
        self.protocol_version = 1


if __name__ == "__main__":
    MessageTest().main()
