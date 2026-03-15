use axum::response::sse::Event;
use futures::stream::Stream;
use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

/// SSE stream wrapper for chat completions
pub struct SseStream {
	rx: mpsc::Receiver<Result<Event, Infallible>>,
}

impl Stream for SseStream {
	type Item = Result<Event, Infallible>;

	fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
		self.rx.poll_recv(cx)
	}
}

/// Create an SSE response with a channel-based stream
pub fn create_sse_stream() -> (mpsc::Sender<Result<Event, Infallible>>, SseStream) {
	let (tx, rx) = mpsc::channel(256);
	(tx, SseStream { rx })
}

/// Send an SSE data event
pub async fn send_sse_data(
	tx: &mpsc::Sender<Result<Event, Infallible>>,
	data: &str,
	event_id: u64,
) -> Result<(), mpsc::error::SendError<Result<Event, Infallible>>> {
	let event = Event::default()
		.data(data)
		.id(event_id.to_string());
	tx.send(Ok(event)).await
}

/// Send an SSE error event
pub async fn send_sse_error(
	tx: &mpsc::Sender<Result<Event, Infallible>>,
	error_data: &str,
	event_id: u64,
) -> Result<(), mpsc::error::SendError<Result<Event, Infallible>>> {
	let event = Event::default()
		.event("error")
		.data(error_data)
		.id(event_id.to_string());
	tx.send(Ok(event)).await
}

/// Send the [DONE] event to close the stream
pub async fn send_sse_done(
	tx: &mpsc::Sender<Result<Event, Infallible>>,
) -> Result<(), mpsc::error::SendError<Result<Event, Infallible>>> {
	let event = Event::default().data("[DONE]");
	tx.send(Ok(event)).await
}

/// Send a keepalive comment
pub async fn send_sse_keepalive(
	tx: &mpsc::Sender<Result<Event, Infallible>>,
) -> Result<(), mpsc::error::SendError<Result<Event, Infallible>>> {
	let event = Event::default().comment("ping");
	tx.send(Ok(event)).await
}
