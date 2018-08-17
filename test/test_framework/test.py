#!/usr/bin/env python3
from test_node import TestNode

x = TestNode(0, "/tmp", "127.0.0.1", "../../target/debug/conflux")
x.start()
x.wait_for_rpc_connection()
print(x.getbestblockhash())
