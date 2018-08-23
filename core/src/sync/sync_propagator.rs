use super::{SyncState, NEW_BLOCK_PACKET};

use bytes::Bytes;
use ethereum_types::{H256, U256};
use network::PeerId;
use rlp::RlpStream;
use sync_ctx::SyncContext;
use types::*;

/// The Ledger Sync Propagator: propagates data to peers
pub struct SyncPropagator;

impl SyncPropagator {
    /// propagates new latest block to a set of peers
    pub fn propagate_new_blocks(
        sync: &mut SyncState, io: &mut SyncContext, blocks: &[H256],
        total_difficulties: &[U256],
    ) -> usize
    {
        trace!(target: "sync", "Sending NewBlocks to peers.");
        let mut sent = 0;

        if blocks.len() != total_difficulties.len() {
            return sent;
        }

        let block_count = blocks.len();

        for peer_id in sync.peers.keys() {
            let mut i = 0;
            while i != block_count {
                let h = blocks[i];
                let total_diff = total_difficulties[i];
                if let Some(header) = io.ledger().block_header(BlockId::Hash(h))
                {
                    if let Some(body) = io.ledger().block_body(BlockId::Hash(h))
                    {
                        let mut rlp_stream = RlpStream::new_list(4);
                        rlp_stream.append(&(NEW_BLOCK_PACKET as u32));
                        rlp_stream.append(&total_diff);
                        let mut data = Bytes::new();
                        data.append(&mut header.rlp());
                        data.append(&mut body.rlp());
                        rlp_stream.append_raw(&data, 2);
                        SyncPropagator::send_packet(
                            io,
                            *peer_id,
                            rlp_stream.out(),
                        );
                    }
                }
                i = i + 1;
            }
            sent += 1;
        }

        sent
    }

    /// Generic packet sender
    fn send_packet(io: &mut SyncContext, peer_id: PeerId, packet: Bytes) {
        if let Err(e) = io.send(peer_id, packet) {
            debug!(target:"sync", "Error sending packet: {:?}", e);
            io.disconnect_peer(peer_id);
        }
    }
}
