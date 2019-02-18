#!/usr/bin/env python3
from http.client import CannotSendRequest

from eth_utils import decode_hex
from rlp.sedes import Binary, BigEndianInt

from conflux import utils
from conflux.utils import encode_hex, bytes_to_int, privtoaddr, parse_as_int, pubtoaddr
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


class P2PTest(ConfluxTestFramework):
    def set_test_params(self):
        self.setup_clean_chain = True
        self.num_nodes = 15
        self.conf_parameters = {"generate-tx": "true", "generate-tx-period-ms": "7", "log-level": "\"debug\"",
                                "storage-cache-size": "20_000_000",
                                "storage-cache-start-size": "20_000_000",}

    def setup_network(self):
        node_per_host = 1
        with open("ips", 'r') as ip_file:
            for line in ip_file.readlines():
                line = line[:-1]
                self.add_remote_nodes(node_per_host, user="ec2-user", ip=line)
        for i in range(len(self.nodes)):
            self.log.info("Node "+str(i) + " bind to "+self.nodes[i].ip+":"+self.nodes[i].port)
        self.start_nodes()
        self.log.info("All nodes started, waiting to be connected")
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
        self.log.info("All nodes connected, waiting to sync")
        sync_blocks(self.nodes)

    def run_test(self):

        self.nodes[0].add_p2p_connection(RemoteNode())
        network_thread_start()
        self.nodes[0].p2p.wait_for_status()
        for i in range(self.num_nodes):
            pub_key = self.nodes[i].key
            addr = self.nodes[i].addr
            self.log.info("%d has addr=%s pubkey=%s", i, encode_hex(addr), pub_key)
            init_tx = create_transaction(value=int(default_config["TOTAL_COIN"]/self.num_nodes), receiver=addr, nonce=i)
            self.nodes[0].p2p.send_protocol_msg(Transactions(transactions=[init_tx]))
        self.nodes[0].disconnect_p2ps()
        block_number = 10000000
        threads = {}
        generate_period = 2
        tx_n = 100000
        for i in range(1, block_number):
            wait_sec = random.expovariate(1 / generate_period)
            p = random.randint(0, self.num_nodes - 1)
            self.log.debug("%d try to generate block", p)
            start = time.time()
            if threads.get(p) is not None:
                threads[p].join()
            thread = GenerateThread(self.nodes, p, tx_n, self.log)
            thread.start()
            threads[p] = thread
            end = time.time()
            if end - start < wait_sec:
                self.log.debug("%d generating block %s", p, str(end-start))
                time.sleep(wait_sec - (end - start))
            else:
                self.log.debug("%d generating block slowly %s", p, str(end-start))
            if i % 1000 == 0:
                for t in threads.values():
                    t.join(60)
                # wait_for_block_count(self.nodes[0], i)
                while True:
                    try:
                        sync_blocks(self.nodes, timeout=60)
                        break
                    except CannotSendRequest as e:
                        time.sleep(1)
                        self.log.warn(e)
                        continue
                self.log.info("%d blocks generated and synced", self.nodes[0].getblockcount())
        self.log.info("Pass")


class RemoteNode(P2PInterface):
    def __init__(self):
        super().__init__(remote=True)
        self.protocol = b'cfx'
        self.protocol_version = 1


class ConnectThread(threading.Thread):
    def __init__(self, nodes, a, peers, latencies):
        threading.Thread.__init__(self, daemon=True)
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


class GenerateThread(threading.Thread):
    def __init__(self, nodes, i, tx_n, log):
        threading.Thread.__init__(self, daemon=True)
        self.nodes = nodes
        self.i = i
        self.tx_n = tx_n
        self.log = log

    def run(self):
        try:
            h = self.nodes[self.i].generateoneblock(self.tx_n)
            self.log.debug("node %d actually generate block %s", self.i, h)
        except Exception as e:
            self.log.error("Node %d fails to generate block", self.i)
            self.log.error(str(e))


if __name__ == "__main__":
    P2PTest().main()
