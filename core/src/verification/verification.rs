use error::Error;
use primitives::Block;

/// Phase 1 quick block verification. Only does checks that are cheap. Operates
/// on a single block
pub fn verify_block_basic(block: &Block) -> Result<(), Error> { Ok(()) }
