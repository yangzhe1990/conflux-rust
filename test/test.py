#!/usr/bin/env python3
from test_framework.test_node import TestNode
from test_framework.util import (PortSeed, p2p_port, get_datadir_path,
                                 initialize_datadir)
from time import sleep
import tempfile
import os

PortSeed.n = os.getpid()

tmpdir = tempfile.mkdtemp(prefix="test")
nodes = []
for i in range(2):
    initialize_datadir(tmpdir, i)
    nodes.append(
        TestNode(
            i,
            get_datadir_path(tmpdir, i),
            rpchost="localhost",
            confluxd=os.path.join(
                os.path.dirname(os.path.realpath(__file__)),
                "../target/debug/conflux")))

nodes[0].start()
nodes[1].start()
nodes[0].wait_for_rpc_connection()
nodes[1].wait_for_rpc_connection()
print(nodes[0].get_block_count())
nodes[0].generate(1)
print(nodes[0].get_block_count())
nodes[0].generate(2)
print(nodes[0].get_block_count())
nodes[0].generate(3)
print(nodes[0].get_block_count())
print(nodes[0].get_best_block_hash())
print(nodes[1].get_block_count())
print(nodes[1].get_best_block_hash())
ip_port = "127.0.0.1:" + str(p2p_port(1))
nodes[0].add_peer(ip_port)
sleep(5)
print("After sleep!")
print(nodes[1].get_block_count())
print(nodes[1].get_best_block_hash())
