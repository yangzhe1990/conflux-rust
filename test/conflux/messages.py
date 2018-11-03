#!/usr/bin/env python3
"""Conflux P2P network half-a-node.

`P2PConnection: A low-level connection object to a node's P2P interface
P2PInterface: A high-level interface object for communicating to a node over P2P
"""

import asyncore
from collections import defaultdict
from io import BytesIO
import rlp
from rlp.exceptions import ObjectSerializationError, ObjectDeserializationError
from rlp.sedes import binary, big_endian_int, CountableList, boolean
import logging
import socket
import struct
import sys
import threading

from conflux import trie
from conflux.config import default_config
from conflux.transactions import Transaction
from conflux.utils import hash32, hash20, sha3
from test_framework.util import wait_until

logger = logging.getLogger("TestFramework.mininode")

PACKET_HELLO = 0x80
PACKET_DISCONNECT = 0x01
PACKET_PING = 0x02
PACKET_PONG = 0x03
PACKET_PROTOCOL = 0x10

BLOCKS = 0x0d
BLOCK_BODIES = 0x08
BLOCK_HASHES = 0x04
BLOCK_HEADERS = 0x06
GET_BLOCKS = 0x0c
GET_BLOCK_BODIES = 0x07
GET_BLOCK_HASHES = 0x03
GET_BLOCK_HEADERS = 0x05
GET_TERMINAL_BLOCK_HASHES = 0x0b
NEW_BLOCK = 0x09
NEW_BLOCK_HASHES = 0x01
STATUS = 0x00
TERMINAL_BLOCK_HASHES = 0x0a
TRANSACTIONS = 0x02


class Capability(rlp.Serializable):
    fields = [
        ("protocol", binary),
        ("version", big_endian_int)
    ]


class NodeEndpoint(rlp.Serializable):
    fields = [
        ("address", binary),
        ("udp_port", big_endian_int),
        ("port", big_endian_int)
    ]


class Hello(rlp.Serializable):
    fields = [
        ("capabilities", CountableList(Capability)),
        ("node_endpoint", NodeEndpoint)
    ]


class Disconnect(rlp.Serializable):
    fields = [
        ("reason", big_endian_int)
    ]


class Status(rlp.Serializable):
    fields = [
        ("protocol_version", big_endian_int),
        ("network_id", big_endian_int),
        ("best_block_hash", hash32),
        ("genesis_hash", hash32)
    ]


class BlockHash(rlp.Serializable):
    fields = [
        ("number", big_endian_int),
        ("hash", hash32)
    ]


class NewBlockHashes:
    def __init__(self, block_hashes=[]):
        assert is_sequence(block_hashes)
        self.block_hashes = block_hashes

    @classmethod
    def serializable(cls, obj):
        if is_sequence(obj.block_hashes):
            return True
        else:
            return False

    @classmethod
    def serialize(cls, obj):
        return CountableList(BlockHash).serialize(obj.block_hashes)

    @classmethod
    def deserialize(cls, serial):
        return cls(block_hashes=CountableList(BlockHash).deserialize(serial))


class Transactions:
    def __init__(self, transactions=[]):
        assert is_sequence(transactions)
        self.transactions = transactions

    @classmethod
    def serializable(cls, obj):
        if is_sequence(obj.transactions):
            return True
        else:
            return False

    @classmethod
    def serialize(cls, obj):
        return CountableList(Transaction).serialize(obj.transactions)

    @classmethod
    def deserialize(cls, serial):
        return cls(transactions=CountableList(Transaction).deserialize(serial))


class GetBlockHashes(rlp.Serializable):
    fields = [
        ("hash", hash32),
        ("max_blocks", big_endian_int)
    ]


class BlockHashes:
    def __init__(self, hashes=[]):
        assert is_sequence(hashes)
        self.hashes = hashes

    @classmethod
    def serializable(cls, obj):
        if is_sequence(obj.hashes):
            return True
        else:
            return False

    @classmethod
    def serialize(cls, obj):
        return CountableList(hash32).serialize(obj.hashes)

    @classmethod
    def deserialize(cls, serial):
        return cls(hashes=CountableList(hash32).deserialize(serial))


class GetBlockHeaders(rlp.Serializable):
    fields = [
        ("hash", hash32),
        ("max_blocks", big_endian_int),
    ]


class BlockHeader(rlp.Serializable):
    fields = [
        ("parent_hash", hash32),
        ("timestamp", big_endian_int),
        ("author", hash20),
        ("transactions_root", hash32),
        ("deferred_state_root", hash32),
        ("difficulty", big_endian_int),
        ("referee_hashes", CountableList(hash32))
    ]

    def __init__(self,
                 parent_hash=default_config['GENESIS_PREVHASH'],
                 timestamp=0,
                 author=default_config['GENESIS_COINBASE'],
                 transactions_root=trie.BLANK_ROOT,
                 deferred_state_root=trie.BLANK_ROOT,
                 difficulty=default_config['GENESIS_DIFFICULTY'],
                 referee_hashes=[]):
        # at the beginning of a method, locals() is a dict of all arguments
        fields = {k: v for k, v in locals().items() if
                  k not in ['self', '__class__']}
        self.block = None
        super(BlockHeader, self).__init__(**fields)

    @property
    def hash(self):
        return sha3(rlp.encode(self))


# class BlockHeaders(CountableList(BlockHeader)):
#     fields = [
#         ("headers", CountableList(BlockHeader))
#     ]
class BlockHeaders:
    def __init__(self, headers=[]):
        assert is_sequence(headers)
        self.headers = headers

    @classmethod
    def serializable(cls, obj):
        if is_sequence(obj.headers):
            return True
        else:
            return False

    @classmethod
    def serialize(cls, obj):
        return CountableList(BlockHeader).serialize(obj.headers)

    @classmethod
    def deserialize(cls, serial):
        return cls(headers=CountableList(BlockHeader).deserialize(serial))


class GetBlockBodies:
    def __init__(self, hashes=[]):
        assert is_sequence(hashes)
        self.hashes = hashes

    @classmethod
    def serializable(cls, obj):
        if is_sequence(obj.hashes):
            return True
        else:
            return False

    @classmethod
    def serialize(cls, obj):
        return CountableList(hash32).serialize(obj.hashes)

    @classmethod
    def deserialize(cls, serial):
        return cls(hashes=CountableList(hash32).deserialize(serial))


class Block(rlp.Serializable):
    fields = [
        ("block_header", BlockHeader),
        ("transactions", CountableList(Transaction))
    ]

    def __init__(self, block_header, transactions=None):
        super(Block, self).__init__(
            block_header=block_header,
            transactions=(transactions or []),
        )


class BlockBodies:
    def __init__(self, bodies=[]):
        assert is_sequence(bodies)
        self.bodies = bodies

    @classmethod
    def serializable(cls, obj):
        if is_sequence(obj.bodies):
            return True
        else:
            return False

    @classmethod
    def serialize(cls, obj):
        return CountableList(Block).serialize(obj.bodies)

    @classmethod
    def deserialize(cls, serial):
        return cls(bodies=CountableList(Block).deserialize(serial))


class NewBlock(rlp.Serializable):
    def __init__(self, block):
        self.block = block

    @classmethod
    def serializable(cls, obj):
            return True

    @classmethod
    def serialize(cls, obj):
        return Block.serialize(obj.block)

    @classmethod
    def deserialize(cls, serial):
        return cls(block=Block.deserialize(serial))


# class TerminalBlockHashes(rlp.Serializable):
#     fields = [
#         ("hashes", CountableList(hash32))
#     ]
class TerminalBlockHashes:
    def __init__(self, hashes=[]):
        assert is_sequence(hashes)
        self.hashes = hashes

    @classmethod
    def serializable(cls, obj):
        if is_sequence(obj.hashes):
            return True
        else:
            return False

    @classmethod
    def serialize(cls, obj):
        return CountableList(hash32).serialize(obj.hashes)

    @classmethod
    def deserialize(cls, serial):
        return cls(hashes=CountableList(hash32).deserialize(serial))


# class GetTerminalBlockHashes(rlp.Serializable):
#     fields = [
#
#     ]
#
#     def serialize(self, obj):
#         if not obj:
#             raise ObjectSerializationError(obj=obj, message="GetTerminalBlockHashes is not an empty object")
#         return []
#
#     def deserialize(self, serial):
#         if not serial:
#             raise ObjectDeserializationError(serial=serial, message="GetTerminalBlockHashes is not an empty object")
#         return GetTerminalBlockHashes()
class GetTerminalBlockHashes:
    def __init__(self):
        pass

    @classmethod
    def serializable(cls, obj):
            return True

    @classmethod
    def serialize(cls, obj):
        return []

    @classmethod
    def deserialize(cls, serial):
        assert not serial
        return cls()


class GetBlocks:
    def __init__(self, hashes=[]):
        assert is_sequence(hashes)
        self.hashes = hashes

    @classmethod
    def serializable(cls, obj):
        if is_sequence(obj.hashes):
            return True
        else:
            return False

    @classmethod
    def serialize(cls, obj):
        return CountableList(hash32).serialize(obj.hashes)

    @classmethod
    def deserialize(cls, serial):
        return cls(hashes=CountableList(hash32).deserialize(serial))


class Blocks:
    def __init__(self, blocks=[]):
        assert is_sequence(blocks)
        self.blocks = blocks

    @classmethod
    def serializable(cls, obj):
        if is_sequence(obj.blocks):
            return True
        else:
            return False

    @classmethod
    def serialize(cls, obj):
        return CountableList(Block).serialize(obj.blocks)

    @classmethod
    def deserialize(cls, serial):
        return cls(blocks=CountableList(Block).deserialize(serial))


msg_id = {
    Status: STATUS,
    NewBlockHashes: NEW_BLOCK_HASHES,
    Transactions: TRANSACTIONS,
    GetBlockHashes: GET_BLOCK_HASHES,
    BlockHashes: BLOCK_HASHES,
    GetBlockHeaders: GET_BLOCK_HEADERS,
    BlockHeaders: BLOCK_HEADERS,
    GetBlockBodies: GET_BLOCK_BODIES,
    BlockBodies: BLOCK_BODIES,
    NewBlock: NEW_BLOCK,
    TerminalBlockHashes: TERMINAL_BLOCK_HASHES,
    GetTerminalBlockHashes: GET_TERMINAL_BLOCK_HASHES,
    GetBlocks: GET_BLOCKS,
    Blocks: BLOCKS
}

msg_class = {}
for c in msg_id:
    msg_class[msg_id[c]] = c


def get_msg_id(msg):
    return msg_id[msg.__class__]


def get_msg_class(msg):
    return msg_class[msg]


def is_sequence(s):
    return isinstance(s, list) or isinstance(s, tuple)
