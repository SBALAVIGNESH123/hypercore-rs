use crate::core::state::RequestTimeline;
use opentelemetry::trace::{Span, SpanBuilder, TraceContextExt, Tracer};
use opentelemetry::{global, Context};
use std::time::{Duration, UNIX_EPOCH};

use opentelemetry_sdk::trace::SdkTracerProvider;

pub fn init_tracer() {
    let provider = SdkTracerProvider::builder().build();
    global::set_tracer_provider(provider);
}

pub fn export_timeline(_session_id: u64, timeline: &RequestTimeline) {
    let tracer = global::tracer("hypercore");

    let queued_at = timeline.queued_at.map(|ts| UNIX_EPOCH + Duration::from_millis(ts));
    let admitted_at = timeline.admitted_at.map(|ts| UNIX_EPOCH + Duration::from_millis(ts));
    let first_token = timeline.first_token_at.map(|ts| UNIX_EPOCH + Duration::from_millis(ts));
    let end_at = timeline.last_token_at
        .or(timeline.cancelled_at)
        .or(timeline.dropped_at)
        .or(timeline.failed_at)
        .map(|ts| UNIX_EPOCH + Duration::from_millis(ts));

    if let (Some(queued), Some(end)) = (queued_at, end_at) {
        let root_builder = SpanBuilder::from_name("inference_request")
            .with_start_time(queued);
        
        let mut root_span = tracer.build(root_builder);
        root_span.end_with_timestamp(end);

        let cx = Context::current_with_span(root_span);

        // Queue Wait Span
        if let Some(admitted) = admitted_at {
            let wait_builder = SpanBuilder::from_name("request_queue_wait")
                .with_start_time(queued);
            let mut wait_span = tracer.build_with_context(wait_builder, &cx);
            wait_span.end_with_timestamp(admitted);
        }

        // Decode / TTFT Span
        if let (Some(admitted), Some(first)) = (admitted_at, first_token) {
            let decode_builder = SpanBuilder::from_name("request_decode")
                .with_start_time(admitted);
            let mut decode_span = tracer.build_with_context(decode_builder, &cx);
            decode_span.end_with_timestamp(first);
        }

        // Streaming Span
        if let (Some(first), Some(last)) = (first_token, end_at) {
            if first < last {
                let stream_builder = SpanBuilder::from_name("request_stream")
                    .with_start_time(first);
                let mut stream_span = tracer.build_with_context(stream_builder, &cx);
                stream_span.end_with_timestamp(last);
            }
        }

        // Cancelled Span
        if let Some(cancelled) = timeline.cancelled_at.map(|ts| UNIX_EPOCH + Duration::from_millis(ts)) {
            let cancel_builder = SpanBuilder::from_name("request_cancel")
                .with_start_time(cancelled);
            let mut cancel_span = tracer.build_with_context(cancel_builder, &cx);
            cancel_span.end_with_timestamp(cancelled);
        }
    }
}
