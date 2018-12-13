#!/usr/bin/env python3
from eth_utils import decode_hex
from rlp.sedes import Binary, BigEndianInt

from conflux import utils
from conflux.utils import encode_hex, bytes_to_int, privtoaddr, parse_as_int
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
        self.num_nodes = 10

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
            t = ConnectThread(self.nodes, i, peer[i], latencies, self.log)
            t.start()
            threads.append(t)
        for t in threads:
            t.join(30)
        sync_blocks(self.nodes)

    def run_test(self):

        for node in self.nodes:
            node.add_p2p_connection(DefaultNode())
        network_thread_start()
        for node in self.nodes:
            node.p2p.wait_for_status()

        genesis_key = default_config["GENESIS_PRI_KEY"]
        balance_map = {genesis_key: 1000000000}
        self.log.info("Initial State: (sk:%d, addr:%s, balance:%d)", bytes_to_int(genesis_key),
                      eth_utils.encode_hex(privtoaddr(genesis_key)), balance_map[genesis_key])
        nonce_map = {genesis_key: 0}

        '''Check if transaction from uncommitted new address can be accepted'''
        tx_n = 5
        receiver_sk = genesis_key
        for i in range(tx_n):
            sender_key = receiver_sk
            value = int(balance_map[sender_key] * random.random())
            nonce = nonce_map[sender_key]
            receiver_sk, _ = ec_random_keys()
            nonce_map[receiver_sk] = 0
            balance_map[receiver_sk] = value
            tx = create_transaction(pri_key=sender_key, receiver=privtoaddr(receiver_sk), value=value, nonce=nonce,
                                    gas_price=100)
            r = random.randint(0, self.num_nodes - 1)
            self.nodes[r].p2p.send_protocol_msg(Transactions(transactions=[tx]))
            nonce_map[sender_key] = nonce + 1
            balance_map[sender_key] -= value
            self.log.debug("New tx %s: %s send value %d to %s, sender balance:%d, receiver balance:%d", encode_hex(tx.hash), eth_utils.encode_hex(privtoaddr(sender_key))[-4:],
                           value, eth_utils.encode_hex(privtoaddr(receiver_sk))[-4:], balance_map[sender_key], balance_map[receiver_sk])
            self.log.debug("Send Transaction %s to node %d", encode_hex(tx.hash), r)
            time.sleep(random.random() / 10)
        block_gen_thread = BlockGenThread(self.nodes, self.log, random.random())
        block_gen_thread.start()
        for k in balance_map:
            self.log.info("Check account sk:%s addr:%s", bytes_to_int(k), eth_utils.encode_hex(privtoaddr(k)))
            wait_until(lambda: self.check_account(k, balance_map))
        self.log.info("Pass 1")

        '''Test Random Transactions'''
        tx_n = 1000
        self.log.info("start to generate %d transactions with about %d seconds", tx_n, tx_n/10/2)
        for i in range(tx_n):
            sender_key = random.choice(list(balance_map))
            value = int(balance_map[sender_key] * random.random())
            # not enough transaction fee (gas_price * gas_limit)
            if balance_map[sender_key] < value + 100:
                continue
            nonce = nonce_map[sender_key]
            if random.random() < 0.1:
                receiver_sk, _ = ec_random_keys()
                nonce_map[receiver_sk] = 0
                balance_map[receiver_sk] = value
            else:
                receiver_sk = random.choice(list(balance_map))
                balance_map[receiver_sk] += value
            tx = create_transaction(pri_key=sender_key, receiver=privtoaddr(receiver_sk), value=value, nonce=nonce,
                                    gas_price=100)
            r = random.randint(0, self.num_nodes - 1)
            self.nodes[r].p2p.send_protocol_msg(Transactions(transactions=[tx]))
            nonce_map[sender_key] = nonce + 1
            balance_map[sender_key] -= value
            self.log.debug("New tx %s: %s send value %d to %s, sender balance:%d, receiver balance:%d nonce:%d", encode_hex(tx.hash), eth_utils.encode_hex(privtoaddr(sender_key))[-4:],
                          value, eth_utils.encode_hex(privtoaddr(receiver_sk))[-4:], balance_map[sender_key], balance_map[receiver_sk], nonce)
            self.log.debug("Send Transaction %s to node %d", encode_hex(tx.hash), r)
            time.sleep(random.random() / 10)
        for k in balance_map:
            self.log.info("Account %s with balance:%s", bytes_to_int(k), balance_map[k]);
        for k in balance_map:
            self.log.info("Check account sk:%s addr:%s", bytes_to_int(k), eth_utils.encode_hex(privtoaddr(k)))
            wait_until(lambda: self.check_account(k, balance_map))
        block_gen_thread.stop()
        block_gen_thread.join()
        self.log.info("Pass")

    def check_account(self, k, balance_map):
        addr = eth_utils.encode_hex(privtoaddr(k))
        try:
            balance = parse_as_int(self.nodes[0].getbalance(addr))
        except Exception as e:
            self.log.info("Fail to get balance, error=%s", str(e))
            return False
        if balance == balance_map[k]:
            return True
        else:
            self.log.info("Remote balance:%d, local balance:%d", balance, balance_map[k])
            time.sleep(1)
            return False


class DefaultNode(P2PInterface):
    def __init__(self):
        super().__init__()
        self.protocol = b'cfx'
        self.protocol_version = 1


class BlockGenThread(threading.Thread):
    def __init__(self, nodes, log, seed):
        threading.Thread.__init__(self)
        self.nodes = nodes
        self.log = log
        self.local_random = random.Random()
        self.local_random.seed(seed)
        self.stopped = False

    def run(self):
        while not self.stopped:
            try:
                r = self.local_random.randint(0, len(self.nodes) - 1)
                h = self.nodes[r].generateoneblock()
                self.log.debug("%s generate block %s", r, h)
                time.sleep(self.local_random.random())
            except Exception as e:
                self.log.info("Fails to generate blocks")
                self.log.info(e)

    def stop(self):
        self.stopped = True


class ConnectThread(threading.Thread):
    def __init__(self, nodes, a, peers, latencies, log):
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
            self.log.error("Node "+str(self.a)+" fails to be connected to" + str(self.peers))
            self.log.error(e)


if __name__ == "__main__":
    P2PTest().main()
