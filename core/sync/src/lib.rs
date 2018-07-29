
/// Represents what has to be handled by actor listening to chain events
pub trait ChainNotify : Send + Sync {
	/// fires when chain has new blocks.
	fn new_blocks(
		&self,
	) {
		// does nothing by default
	}

	/// fires when chain achieves active mode
	fn start(&self) {
		// does nothing by default
	}

	/// fires when chain achieves passive mode
	fn stop(&self) {
		// does nothing by default
	}

	/// fires when chain broadcasts a message
	fn broadcast(&self,) {}

	/// fires when new transactions are received from a peer
	fn transactions_received(&self,
	) {
		// does nothing by default
	}
}
