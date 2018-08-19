#!/usr/bin/env python3
from test_node import TestNode

x = TestNode(0, "/tmp", "127.0.0.1", 31001, 32001, 33001, "../../target/debug/conflux")
y = TestNode(1, "/tmp", "127.0.0.1", 31002, 32002, 33002, "../../target/debug/conflux")
x.start()
y.start()
x.wait_for_rpc_connection()
y.wait_for_rpc_connection()
print(x.getblockcount())
x.generate(1)
print(x.getblockcount())
x.generate(2)
print(x.getblockcount())
x.generate(3)
print(x.getblockcount())
print(x.getbestblockhash())
print(y.getblockcount())
print(y.getbestblockhash())
