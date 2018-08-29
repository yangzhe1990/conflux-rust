use ethereum_types::{Address, H160, H256, U256};
use ethkey::{self, public_to_address, recover, Public, Secret, Signature};
use hash::keccak;
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use std::mem::*;
use std::ops::Deref;

/// Fake address for unsigned transactions.
pub const UNSIGNED_SENDER: Address = H160([0xff; 20]);

#[derive(Default, Debug, Clone, PartialEq)]
pub struct Transaction {
    pub nonce: u64,
    pub gas_price: f64,
    pub gas: u64,
    pub value: f64,
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
        let tx_with_sig = self.with_signature(sig);
        let public = tx_with_sig
            .recover_public()
            .expect("secret is valid so it's recoverable");
        SignedTransaction::new(public, tx_with_sig)
    }

    /// Signs the transaction with signature.
    pub fn with_signature(self, sig: Signature) -> TransactionWithSignature {
        TransactionWithSignature {
            unsigned: self,
            r: sig.r().into(),
            s: sig.s().into(),
            v: sig.v(),
            hash: 0.into(),
        }.compute_hash()
    }
}

impl Decodable for Transaction {
    fn decode(r: &Rlp) -> Result<Self, DecoderError> {
        let gas_price_int: u64 = r.val_at(1)?;
        let gas_price = unsafe { transmute::<u64, f64>(gas_price_int) };
        let val_int: u64 = r.val_at(3)?;
        let val = unsafe { transmute::<u64, f64>(val_int) };
        let txn = Transaction {
            nonce: r.val_at(0)?,
            gas_price: gas_price,
            gas: r.val_at(2)?,
            value: val,
            receiver: r.val_at(4)?,
        };
        Ok(txn)
    }
}

impl Encodable for Transaction {
    fn rlp_append(&self, s: &mut RlpStream) {
        let gas_price_int = unsafe { transmute::<f64, u64>(self.gas_price) };
        let val_int = unsafe { transmute::<f64, u64>(self.value) };

        s.begin_list(5);
        s.append(&self.nonce);
        s.append(&gas_price_int);
        s.append(&self.gas);
        s.append(&val_int);
        s.append(&self.receiver);
    }
}

/// Signed transaction information without verified signature.
#[derive(Debug, Clone, PartialEq)]
pub struct TransactionWithSignature {
    /// Plain Transaction.
    unsigned: Transaction,
    /// The V field of the signature; helps describe which half of the curve our point falls in.
    v: u8,
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
        if d.item_count()? != 8 {
            return Err(DecoderError::RlpIncorrectListLen);
        }
        let hash = keccak(d.as_raw());

        let gas_price_int: u64 = d.val_at(1)?;
        let gas_price = unsafe { transmute::<u64, f64>(gas_price_int) };
        let val_int: u64 = d.val_at(3)?;
        let val = unsafe { transmute::<u64, f64>(val_int) };

        Ok(TransactionWithSignature {
            unsigned: Transaction {
                nonce: d.val_at(0)?,
                gas_price: gas_price,
                gas: d.val_at(2)?,
                value: val,
                receiver: d.val_at(4)?,
            },
            v: d.val_at(5)?,
            r: d.val_at(6)?,
            s: d.val_at(7)?,
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
        let gas_price_int = unsafe { transmute::<f64, u64>(self.gas_price) };
        let val_int = unsafe { transmute::<f64, u64>(self.value) };

        s.begin_list(8);
        s.append(&self.nonce);
        s.append(&gas_price_int);
        s.append(&self.gas);
        s.append(&val_int);
        s.append(&self.receiver);
        s.append(&self.v);
        s.append(&self.r);
        s.append(&self.s);
    }

    /// Construct a signature object from the sig.
    pub fn signature(&self) -> Signature {
        Signature::from_rsv(&self.r.into(), &self.s.into(), self.v)
    }

    pub fn hash(&self) -> H256 { self.hash }

    /// Recovers the public key of the sender.
    pub fn recover_public(&self) -> Result<Public, ethkey::Error> {
        Ok(recover(&self.signature(), &self.unsigned.hash())?)
    }
}

/// A signed transaction with successfully recovered `sender`.
#[derive(Debug, Clone, PartialEq)]
pub struct SignedTransaction {
    pub transaction: TransactionWithSignature,
    pub sender: Address,
    public: Option<Public>,
}

impl Encodable for SignedTransaction {
    fn rlp_append(&self, s: &mut RlpStream) {
        self.transaction.rlp_append_sealed_transaction(s)
    }
}

impl Deref for SignedTransaction {
    type Target = TransactionWithSignature;
    fn deref(&self) -> &Self::Target { &self.transaction }
}

impl From<SignedTransaction> for TransactionWithSignature {
    fn from(tx: SignedTransaction) -> Self { tx.transaction }
}

impl SignedTransaction {
    /// Try to verify transaction and recover sender.
    pub fn new(public: Public, transaction: TransactionWithSignature) -> Self {
        if transaction.is_unsigned() {
            SignedTransaction {
                transaction: transaction,
                sender: UNSIGNED_SENDER,
                public: None,
            }
        } else {
            let sender = public_to_address(&public);
            SignedTransaction {
                transaction: transaction,
                sender: sender,
                public: Some(public),
            }
        }
    }

    /// Returns transaction sender.
    pub fn sender(&self) -> Address { self.sender }

    /// Checks if signature is empty.
    pub fn is_unsigned(&self) -> bool { self.transaction.is_unsigned() }
}
