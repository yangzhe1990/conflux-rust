#!/usr/bin/env python3
from conflux.config import default_config
from conflux.messages import BlockHeader, Block
from conflux.transactions import Transaction


def create_block(parent_hash, timestamp, difficulty=1):
    block = Block(BlockHeader(parent_hash=parent_hash, difficulty=difficulty, timestamp=timestamp))
    return block


def create_transaction(nonce=0, gas_price=1, gas=100, value=0, receiver=default_config['GENESIS_COINBASE'],
                       v=0, r=0, s=0):
    transaction = Transaction(nonce, gas_price, gas, value, receiver, v, r, s)
    return transaction
