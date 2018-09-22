import rlp
from .utils import (
    normalize_address, hash32, trie_root, big_endian_int, address, int256,
    encode_hex, decode_hex, encode_int, sha3
)
from rlp.sedes import big_endian_int, Binary, binary, CountableList
from . import trie
from . import utils
from .config import default_config
from .transactions import Transaction
import sys


class BlockHeader(rlp.Serializable):
    fields = [
        ('prevhash', hash32),
        ('uncles_hash', hash32),
        ('coinbase', address),
        ('state_root', trie_root),
        ('tx_list_root', trie_root),
        ('receipts_root', trie_root),
        ('difficulty', big_endian_int),
        ('number', big_endian_int),
        ('gas_limit', big_endian_int),
        ('gas_used', big_endian_int),
        ('timestamp', big_endian_int),
        ('extra_data', binary),
        ('nonce', binary)
    ]

    def __init__(self,
                 prevhash=default_config['GENESIS_PREVHASH'],
                 uncles_hash=utils.sha3rlp([]),
                 coinbase=default_config['GENESIS_COINBASE'],
                 state_root=trie.BLANK_ROOT,
                 tx_list_root=trie.BLANK_ROOT,
                 receipts_root=trie.BLANK_ROOT,
                 bloom=0,
                 difficulty=default_config['GENESIS_DIFFICULTY'],
                 number=0,
                 gas_limit=default_config['GENESIS_GAS_LIMIT'],
                 gas_used=0,
                 timestamp=0,
                 extra_data=b'',
                 nonce=b''):
        # at the beginning of a method, locals() is a dict of all arguments
        fields = {k: v for k, v in locals().items() if
                  k not in ['self', '__class__']}
        if len(fields['coinbase']) == 40:
            fields['coinbase'] = decode_hex(fields['coinbase'])
        assert len(fields['coinbase']) == 20
        self.block = None
        super(BlockHeader, self).__init__(**fields)

    @property
    def hash(self):
        """The binary block hash"""
        return utils.sha3(rlp.encode(self))

    @property
    def hex_hash(self):
        return encode_hex(self.hash)

    @property
    def mining_hash(self):
        mining_fields = [
            (field, sedes) for field, sedes in BlockHeader._meta.fields
            if field not in ["nonce"]
        ]

        class MiningBlockHeader(rlp.Serializable):
            fields = mining_fields

        _self = MiningBlockHeader(
            **{f: getattr(self, f) for (f, sedes) in mining_fields})

        return utils.sha3(rlp.encode(
            _self, MiningBlockHeader))

    @property
    def signing_hash(self):
        # exclude extra_data
        signing_fields = [
            (field, sedes) for field, sedes in BlockHeader._meta.fields
            if field not in ["extra_data"]
        ]

        class SigningBlockHeader(rlp.Serializable):
            fields = signing_fields

        _self = SigningBlockHeader(
            **{f: getattr(self, f) for (f, sedes) in signing_fields})

        return utils.sha3(rlp.encode(
            _self, SigningBlockHeader))

    def to_dict(self):
        """Serialize the header to a readable dictionary."""
        d = {}
        for field in ('prevhash', 'uncles_hash', 'extra_data', 'nonce'):
            d[field] = '0x' + encode_hex(getattr(self, field))
        for field in ('state_root', 'tx_list_root', 'receipts_root',
                      'coinbase'):
            d[field] = encode_hex(getattr(self, field))
        for field in ('number', 'difficulty', 'gas_limit', 'gas_used',
                      'timestamp'):
            d[field] = utils.to_string(getattr(self, field))
        assert len(d) == len(BlockHeader.fields)
        return d

    def __repr__(self):
        return '<%s(#%d %s)>' % (self.__class__.__name__, self.number,
                                 encode_hex(self.hash)[:8])

    def __eq__(self, other):
        """Two blockheaders are equal iff they have the same hash."""
        return isinstance(other, BlockHeader) and self.hash == other.hash

    def __hash__(self):
        return utils.big_endian_to_int(self.hash)

    def __ne__(self, other):
        return not self.__eq__(other)


class Block(rlp.Serializable):
    fields = [
        ('header', BlockHeader),
        ('transactions', CountableList(Transaction)),
    ]

    def __init__(self, header, transactions=None, uncles=None, db=None):
        # assert isinstance(db, BaseDB), "No database object given"
        # self.db = db

        super(Block, self).__init__(
            header=header,
            transactions=(transactions or []),
            uncles=list(uncles or []),
        )

    def __getattribute__(self, name):
        try:
            return rlp.Serializable.__getattribute__(self, name)
        except AttributeError:
            return getattr(self.header, name)

    @property
    def transaction_count(self):
        return len(self.transactions)
