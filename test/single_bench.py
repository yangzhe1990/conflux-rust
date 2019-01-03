#!/usr/bin/env python3
import datetime
from http.client import CannotSendRequest

from conflux.utils import convert_to_nodeid, privtoaddr, parse_as_int
from test_framework.blocktools import  create_transaction
from test_framework.test_framework import ConfluxTestFramework
from test_framework.mininode import *
from test_framework.util import *


class MessageTest(ConfluxTestFramework):
    def set_test_params(self):
        self.setup_clean_chain = True
        self.num_nodes = 1
        self.conf_parameters = {"log-level":"\"error\""}

    def setup_network(self):
        self.setup_nodes(binary=[os.path.join(
            os.path.dirname(os.path.realpath(__file__)),
            "../target/release/conflux")])

    def run_test(self):

        # Start mininode connection
        default_node = DefaultNode()
        self.node = self.nodes[0]
        self.node.add_p2p_connection(default_node)
        network_thread_start()
        default_node.wait_for_status()

        block_gen_thread = BlockGenThread(self.node, self.log, random.random())
        block_gen_thread.start()
        genesis_key = default_config["GENESIS_PRI_KEY"]
        balance_map = {genesis_key: default_config["TOTAL_COIN"]}
        self.log.info("Initial State: (sk:%d, addr:%s, balance:%d)", bytes_to_int(genesis_key),
                      eth_utils.encode_hex(privtoaddr(genesis_key)), balance_map[genesis_key])
        nonce_map = {genesis_key: 0}
        '''Test Random Transactions'''
        all_txs = []
        tx_n = 20000
        gas_price = 1
        self.log.info("start to generate %d transactions", tx_n)
        for i in range(tx_n):
            if i % 1000 == 0:
                self.log.debug("generated %d tx", i)
            sender_key = random.choice(list(balance_map))
            nonce = nonce_map[sender_key]
            if random.random() < 0.1 and balance_map[sender_key] > 21000 * 4 * tx_n:
                value = int(balance_map[sender_key] * 0.5)
                receiver_sk, _ = ec_random_keys()
                nonce_map[receiver_sk] = 0
                balance_map[receiver_sk] = value
            else:
                value = 1
                receiver_sk = random.choice(list(balance_map))
                balance_map[receiver_sk] += value
            # not enough transaction fee (gas_price * gas_limit) should not happen for now
            assert balance_map[sender_key] >= value + gas_price * 21000
            tx = create_transaction(pri_key=sender_key, receiver=privtoaddr(receiver_sk), value=value, nonce=nonce,
                                    gas_price=gas_price)
            all_txs.append(tx)
            nonce_map[sender_key] = nonce + 1
            balance_map[sender_key] -= value + gas_price * 21000
        start_time = datetime.datetime.now()
        i = 0
        for tx in all_txs:
            i += 1
            if i % 1000 == 0:
                self.log.debug("Sent %d tx", i)
            self.node.p2p.send_protocol_msg(Transactions(transactions=[tx]))
        for k in balance_map:
                wait_until(lambda: self.check_account(k, balance_map))
        end_time = datetime.datetime.now()
        time_used = (end_time - start_time).total_seconds()
        block_gen_thread.stop()
        block_gen_thread.join()
        self.log.info("Time used: %f seconds", time_used)
        self.log.info("Tx per second: %f", tx_n / time_used)

    def check_account(self, k, balance_map):
        addr = eth_utils.encode_hex(privtoaddr(k))
        try:
            balance = parse_as_int(self.node.getbalance(addr))
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
    def __init__(self, node, log, seed):
        threading.Thread.__init__(self, daemon=True)
        self.node = node
        self.log = log
        self.local_random = random.Random()
        self.local_random.seed(seed)
        self.stopped = False

    def run(self):
        while not self.stopped:
            try:
                time.sleep(0.2)
                h = self.node.generateoneblock(100000)
                self.log.debug("%s generate block %s", 0, h)
            except Exception as e:
                self.log.info("Fails to generate blocks")
                self.log.info(e)

    def stop(self):
        self.stopped = True


if __name__ == "__main__":
    MessageTest().main()
