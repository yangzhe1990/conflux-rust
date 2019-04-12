#!/usr/bin/env python3
import datetime
from http.client import CannotSendRequest

from conflux.utils import convert_to_nodeid, privtoaddr, parse_as_int, encode_hex
from test_framework.blocktools import  create_transaction
from test_framework.test_framework import ConfluxTestFramework
from test_framework.mininode import *
from test_framework.util import *
import rlp
import numpy

class RlpIter:
    BUFFER_SIZE = 1000000

    def __init__(self, f):
        self.f = f
        self.bytes = bytearray()
        self.eof = False
        self.offset = 0

    def __iter__(self):
        return self

    def __next__(self):
        length = len(self.bytes)
        if not self.eof and length < RlpIter.BUFFER_SIZE * 2:
            to_append = self.f.read(RlpIter.BUFFER_SIZE * 2 - length)
            self.eof = (len(to_append) == 0)
            self.bytes += to_append
            length = len(self.bytes)
        if length > 0:
            try:
                (prefix, type, length, end) = rlp.codec.consume_length_prefix(self.bytes, self.offset)
                old_offset = self.offset
                self.offset += len(prefix) + length
                rlpbytes = self.bytes[old_offset:self.offset]
                if self.offset >= RlpIter.BUFFER_SIZE:
                    self.bytes = self.bytes[RlpIter.BUFFER_SIZE:]
                    self.offset -= RlpIter.BUFFER_SIZE
                return rlpbytes
            except Exception as e:
                print("error parsing rlp.")
                raise StopIteration()
        else:
            raise StopIteration()


class ConfluxEthReplayTest(ConfluxTestFramework):
    EXPECTED_TPS = 3000

    def set_test_params(self):
        self.setup_clean_chain = True
        self.num_nodes = 4
        #self.num_nodes = 1
        self.conf_parameters = {"log_level": "\"debug\"",
                                "storage_cache_start_size": "1000000",
                                "storage_cache_size": "20000000",
                                "storage_node_map_size": "200000000",
                                "ledger_cache_size": "16",
                                "egress_queue_capacity": "1024",
                                "egress_min_throttle": "100",
                                "egress_max_throttle": "1000",}

    def setup_network(self):
        self.remote = True
        ips = [
            "13.93.127.52",
            "40.68.152.173",
            "13.69.11.133",
            "52.233.188.156",
            "40.68.157.246",
        ]
        self.num_nodes = len(ips)
        binary = ["/home/ubuntu/conflux"]

        for ip in ips:
            self.add_remote_nodes(1, user="ubuntu", ip=ip, binary=binary)
        for i in range(len(self.nodes)):
            self.log.info("Node "+str(i) + " bind to "+self.nodes[i].ip+":"+self.nodes[i].port)
        self.start_nodes()
        self.log.info("All nodes started, waiting to be connected")

        """ local nodes
        self.setup_nodes(binary=[os.path.join(
            os.path.dirname(os.path.realpath(__file__)),
            #"../target/debug/conflux")])
            "../target/release/conflux")]*self.num_nodes)
            """

        connect_sample_nodes(self.nodes, self.log, 2, 0, 300)

    def run_test(self):
        # Start mininode connection
        p2p = start_p2p_connection(self.nodes, self.remote)

        #time.sleep(10000)

        """
        for node in self.nodes:
            node.add_p2p_connection(default_node)
        """

        for node in self.nodes:
            block_gen_thread = BlockGenThread(node, self.log, random.random(), 1.0/self.num_nodes)
            block_gen_thread.start()


        TX_FILE_PATH = "/run/media/yangzhe/HDDDATA/conflux_e2e_benchmark/convert_eth_from_0_to_4141811_unknown_txs.rlp"
        f = open(TX_FILE_PATH, "rb")

        start_time = datetime.datetime.now()
        last_log_elapsed_time = 0
        tx_count = 0
        peer_to_send = 0
        for encoded in RlpIter(f):
            #if tx_count % 10000 == 0:
            peer_to_send = (peer_to_send + 1) % self.num_nodes

            txs_rlp = rlp.codec.length_prefix(len(encoded), 192) + encoded

            self.nodes[peer_to_send].p2p.send_protocol_packet(int_to_bytes(
                TRANSACTIONS) + txs_rlp)
            elapsed_time = (datetime.datetime.now() - start_time).total_seconds()

            speed_diff = 1.0 * tx_count / ConfluxEthReplayTest.EXPECTED_TPS - elapsed_time
            if int(elapsed_time - last_log_elapsed_time) >= 1:
                last_log_elapsed_time = elapsed_time
                self.log.info("elapsed time %s, tx_count %s", elapsed_time, tx_count)
            if speed_diff >= 1:
                time.sleep(speed_diff)

            tx_count += 1
        f.close()

        end_time = datetime.datetime.now()
        time_used = (end_time - start_time).total_seconds()
        block_gen_thread.stop()
        block_gen_thread.join()
        self.log.info("Time used: %f seconds", time_used)
        self.log.info("Tx per second: %f", tx_count / time_used)

class DefaultNode(P2PInterface):
    def __init__(self):
        super().__init__()
        self.protocol = b'cfx'
        self.protocol_version = 1


class BlockGenThread(threading.Thread):
    BLOCK_SIZE_LIMIT=40000
    def __init__(self, node, log, seed, hashpower):
        threading.Thread.__init__(self, daemon=True)
        self.node = node
        self.log = log
        self.local_random = random.Random()
        self.local_random.seed(seed)
        self.stopped = False
        self.hashpower_percent = hashpower

    def run(self):
        for i in range(0, 20):
            h = self.node.generateoneblocknonblock(BlockGenThread.BLOCK_SIZE_LIMIT)
            self.log.info("%s generate block at test start %s", 0, h)
            time.sleep(1)
        while not self.stopped:
            try:
                h = self.node.generateoneblocknonblock(BlockGenThread.BLOCK_SIZE_LIMIT)
                mining = 0.8 * numpy.random.exponential() / self.hashpower_percent
                self.log.info("%s generate block %s and sleep %s sec", 0, h, mining)
                time.sleep(mining)
            except Exception as e:
                self.log.info("Fails to generate blocks")
                self.log.info(e)

    def stop(self):
        self.stopped = True


if __name__ == "__main__":
    ConfluxEthReplayTest().main()

# FIXME: print balance for genesis account