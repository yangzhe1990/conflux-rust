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
        self.num_nodes = 16

    def setup_network(self):
        self.setup_nodes()
        peer_n = 3
        peer = [[] for _ in range(self.num_nodes)]
        latencies = [{} for _ in range(self.num_nodes)]
        threads = []
        for i in range(self.num_nodes):
            for _ in range(peer_n):
                while True:
                    p = random.randint(0, self.num_nodes - 1)
                    if p not in peer[i] and not p == i:
                        peer[i].append(p)
                        lat = random.randint(0, 300)
                        latencies[i][p] = lat
                        latencies[p][i] = lat
                        break
        for i in range(self.num_nodes):
            t = ConnectThread(self.nodes, i, peer[i], latencies);
            t.start()
            threads.append(t)
        for t in threads:
            t.join(30)
        sync_blocks(self.nodes)

    def run_test(self):
        block_number = 1000

        for i in range(1, block_number):
            chosen_peer = random.randint(0, self.num_nodes - 1)
            block_hash = self.nodes[chosen_peer].generate(1, 0)
            self.log.info("%d generate block %s", chosen_peer, block_hash)
            time.sleep(random.random()/2)
        wait_for_block_count(self.nodes[0], block_number)
        sync_blocks(self.nodes, timeout=30)
        self.log.info("Pass")


class DefaultNode(P2PInterface):
    def __init__(self):
        super().__init__()
        self.protocol = b'cfx'
        self.protocol_version = 1


class ConnectThread(threading.Thread):
    def __init__(self, nodes, a, peers, latencies):
        threading.Thread.__init__(self)
        self.nodes = nodes
        self.a = a
        self.peers = peers
        self.latencies = latencies

    def run(self):
        try:
            while True:
                for i in range(len(self.peers)):
                    p = self.peers[i]
                    connect_nodes(self.nodes, self.a, p)
                for p in self.latencies[self.a]:
                    self.nodes[self.a].addlatency(self.nodes[p].key, self.latencies[self.a][p])
                if len(self.nodes[self.a].getpeerinfo()) >= 3:
                    break
                else:
                    time.sleep(1)
        except Exception as e:
            print("Node "+str(self.a)+" fails to be connected to" + str(self.peers))
            print(e)


if __name__ == "__main__":
    P2PTest().main()
