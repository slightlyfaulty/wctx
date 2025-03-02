use tokio::sync::mpsc;
use tokio::time::{sleep, Instant, Duration};

/// An async debouncer which, after receiving one or more pushed values,
/// will yield the most-recent value only after no new value has arrived
/// for at least `delay` duration. After yielding, it “resets” and can be awaited
/// again for a new debounced value.
pub struct Debouncer<T> {
	/// Used to push new values.
	input_tx: mpsc::UnboundedSender<T>,
	/// The debounced output.
	output_rx: mpsc::UnboundedReceiver<T>,
}

impl<T: Send + 'static> Debouncer<T> {
	/// Create a new debouncer. The output future resolves only after no new value
	/// is pushed for the specified `delay` period.
	pub fn new(delay: Duration) -> Self {
		// Create an unbounded channel for incoming events.
		let (input_tx, mut input_rx) = mpsc::unbounded_channel();
		// Create an unbounded channel to send out debounced events.
		let (output_tx, output_rx) = mpsc::unbounded_channel();

		// Spawn a background task that performs the debouncing.
		tokio::spawn(async move {
			// Loop as long as new input values keep coming.
			while let Some(first) = input_rx.recv().await {
				// Start a debounce cycle with the first value.
				let mut last = first;
				// Create a timer that will fire after the specified delay.
				let timer = sleep(delay);
				// Pin the timer so that we can call `.reset()`.
				tokio::pin!(timer);

				loop {
					tokio::select! {
                        // If a new value is pushed before the timer expires…
                        maybe = input_rx.recv() => {
                            if let Some(new_val) = maybe {
                                // Update our “latest” value…
                                last = new_val;
                                // …and reset the timer to fire delay from now.
                                timer.as_mut().reset(Instant::now() + delay);
                            } else {
                                // If the channel is closed, exit the inner loop.
                                break;
                            }
                        }
                        // If no new value arrives for `delay`, the timer fires.
                        _ = &mut timer => {
                            break;
                        }
                    }
				}

				// Send out the most recent value.
				// (We ignore errors here because it just means no one is awaiting.)
				let _ = output_tx.send(last);
			}
		});

		Self {
			input_tx,
			output_rx,
		}
	}

	/// Push a new value into the debouncer.
	/// Any pending debounced value will be updated with this value.
	pub fn push(&self, value: T) {
		// We ignore the error (if any) because it just means the background task has finished.
		let _ = self.input_tx.send(value);
	}

	/// Wait for the next debounced value.
	///
	/// Each time you call this, the future is unresolved until a burst of pushes
	/// settles down (i.e. no new push occurs for the given delay), at which point
	/// it yields the latest value.
	pub async fn next(&mut self) -> Option<T> {
		self.output_rx.recv().await
	}
}
