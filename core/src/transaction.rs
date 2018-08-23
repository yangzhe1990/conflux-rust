use ethereum_types::{Address, H160, H256, U256};
use ethkey::{self, public_to_address, recover, Public, Secret, Signature};
use hash::keccak;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use std::mem::*;
use std::ops::Deref;

/// Fake address for unsigned transactions.
pub const UNSIGNED_SENDER: Address = H160([0xff; 20]);

/// Replay protection logic for v part of transaction's signature
pub mod signature {
    pub fn add_chain_replay_protection(v: u64) -> u64 { v + 27 }

    /// Returns refined v
    /// 0 if `v` would have been 27 under "Electrum" notation, 1 if 28 or 4 if invalid.
    pub fn check_replay_protection(v: u64) -> u8 {
        match v {
            v if v == 27 => 0,
            v if v == 28 => 1,
            v if v > 36 => ((v - 1) % 2) as u8,
            _ => 4,
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct Transaction {
    pub nonce: u64,
    pub value: f64,
    pub sender: Address,
    pub receiver: Address,
}

impl Transaction {
    pub fn hash(&self) -> H256 {
        let mut s = RlpStream::new();
        s.append(self);
        keccak(s.as_raw())
    }

    pub fn sign(self, secret: &Secret) -> SignedTransaction {
        let sig = ::ethkey::sign(secret, &self.hash())
            .expect("data is valid and context has signing capabilities; qed");
        SignedTransaction::new(self.with_signature(sig))
            .expect("secret is valid so it's recoverable")
    }

    /// Signs the transaction with signature.
    pub fn with_signature(self, sig: Signature) -> TransactionWithSignature {
        TransactionWithSignature {
            unsigned: self,
            r: sig.r().into(),
            s: sig.s().into(),
            v: signature::add_chain_replay_protection(sig.v() as u64),
            hash: 0.into(),
        }.compute_hash()
    }
}

impl Decodable for Transaction {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        let val_int: u64 = r.val_at(1)?;
        let val = unsafe { transmute::<u64, f64>(val_int) };
        let txn = Transaction {
            nonce: r.val_at(0)?,
            value: val,
            sender: r.val_at(2)?,
            receiver: r.val_at(3)?,
        };
        Ok(txn)
    }
}

impl Encodable for Transaction {
    fn rlp_append(&self, s: &mut RlpStream) {
        s.begin_list(4);
        s.append(&self.nonce);
        let val_int = unsafe { transmute::<f64, u64>(self.value) };
        s.append(&val_int);
        s.append(&self.sender);
        s.append(&self.receiver);
    }
}

/// Signed transaction information without verified signature.
#[derive(Debug, Clone, PartialEq)]
pub struct TransactionWithSignature {
    /// Plain Transaction.
    unsigned: Transaction,
    /// The V field of the signature; the LS bit described which half of the curve our point falls
    /// in. The MS bits describe which chain this transaction is for. If 27/28, its for all chains.
    v: u64,
    /// The R field of the signature; helps describe the point on the curve.
    r: U256,
    /// The S field of the signature; helps describe the point on the curve.
    s: U256,
    /// Hash of the transaction
    hash: H256,
}

impl Deref for TransactionWithSignature {
    type Target = Transaction;

    fn deref(&self) -> &Self::Target { &self.unsigned }
}

impl Decodable for TransactionWithSignature {
    fn decode(d: &Rlp) -> Result<Self, DecoderError> {
        if d.item_count()? != 9 {
            return Err(DecoderError::RlpIncorrectListLen);
        }
        let hash = keccak(d.as_raw());

        let val_int: u64 = d.val_at(1)?;
        let val = unsafe { transmute::<u64, f64>(val_int) };

        Ok(TransactionWithSignature {
            unsigned: Transaction {
                nonce: d.val_at(0)?,
                value: val,
                sender: d.val_at(2)?,
                receiver: d.val_at(3)?,
            },
            v: d.val_at(4)?,
            r: d.val_at(5)?,
            s: d.val_at(6)?,
            hash: hash,
        })
    }
}

impl Encodable for TransactionWithSignature {
    fn rlp_append(&self, s: &mut RlpStream) {
        self.rlp_append_sealed_transaction(s)
    }
}

impl TransactionWithSignature {
    /// Used to compute hash of created transactions
    fn compute_hash(mut self) -> TransactionWithSignature {
        let hash = keccak(&*self.rlp_bytes());
        self.hash = hash;
        self
    }

    /// Checks whether signature is empty.
    pub fn is_unsigned(&self) -> bool { self.r.is_zero() && self.s.is_zero() }

    /// Append object with a signature into RLP stream
    fn rlp_append_sealed_transaction(&self, s: &mut RlpStream) {
        let val_int = unsafe { transmute::<f64, u64>(self.value) };

        s.begin_list(7);
        s.append(&self.nonce);
        s.append(&val_int);
        s.append(&self.sender);
        s.append(&self.receiver);
        s.append(&self.v);
        s.append(&self.r);
        s.append(&self.s);
    }

    pub fn standard_v(&self) -> u8 {
        signature::check_replay_protection(self.v)
    }

    /// Construct a signature object from the sig.
    pub fn signature(&self) -> Signature {
        Signature::from_rsv(&self.r.into(), &self.s.into(), self.standard_v())
    }

    /// Recovers the public key of the sender.
    pub fn recover_public(&self) -> Result<Public, ethkey::Error> {
        Ok(recover(&self.signature(), &self.unsigned.hash())?)
    }
}

/// A signed transaction with successfully recovered `sender`.
#[derive(Debug, Clone, PartialEq)]
pub struct SignedTransaction {
    transaction: TransactionWithSignature,
    sender: Address,
    public: Option<Public>,
}

impl SignedTransaction {
    /// Try to verify transaction and recover sender.
    pub fn new(
        transaction: TransactionWithSignature,
    ) -> Result<Self, ethkey::Error> {
        if transaction.is_unsigned() {
            Ok(SignedTransaction {
                transaction: transaction,
                sender: UNSIGNED_SENDER,
                public: None,
            })
        } else {
            let public = transaction.recover_public()?;
            let sender = public_to_address(&public);
            Ok(SignedTransaction {
                transaction: transaction,
                sender: sender,
                public: Some(public),
            })
        }
    }

    /// Checks is signature is empty.
    pub fn is_unsigned(&self) -> bool { self.transaction.is_unsigned() }
}
