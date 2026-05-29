use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

use crate::metrics::events::{dispatch, LatencyClass, MetricEvent};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RequestState {
    Queued,
    Admitted,
    Active,
    Completed,
    Cancelled,
    Rejected,
    Dropped,
    Failed,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct RequestTimeline {
    pub queued_at: Option<u64>,
    pub admitted_at: Option<u64>,
    pub first_token_at: Option<u64>,
    pub last_token_at: Option<u64>,
    pub cancelled_at: Option<u64>,
    pub dropped_at: Option<u64>,
    pub failed_at: Option<u64>,
    pub current_state: Option<RequestState>,
}

impl RequestTimeline {
    pub fn transition(&mut self, state: RequestState) {
        let now = now_ms();
        self.current_state = Some(state);

        match state {
            RequestState::Queued => {
                self.queued_at = Some(now);
                dispatch(MetricEvent::RequestEnqueued);
            }
            RequestState::Admitted => {
                if let Some(queued) = self.queued_at {
                    dispatch(MetricEvent::LatencyMeasured {
                        duration_ms: now.saturating_sub(queued),
                        class: LatencyClass::QueueWait,
                    });
                }
                self.admitted_at = Some(now);
                dispatch(MetricEvent::RequestAdmitted);
            }
            RequestState::Active => {
                if self.first_token_at.is_none() {
                    if let Some(admitted) = self.admitted_at {
                        dispatch(MetricEvent::LatencyMeasured {
                            duration_ms: now.saturating_sub(admitted),
                            class: LatencyClass::FirstToken,
                        });
                    }
                    self.first_token_at = Some(now);
                }
            }
            RequestState::Completed => {
                if let Some(queued) = self.queued_at {
                    dispatch(MetricEvent::LatencyMeasured {
                        duration_ms: now.saturating_sub(queued),
                        class: LatencyClass::TotalGeneration,
                    });
                }
                self.last_token_at = Some(now);
            }
            RequestState::Cancelled => {
                self.cancelled_at = Some(now);
                dispatch(MetricEvent::RequestCancelled);
            }
            RequestState::Rejected => {
                self.dropped_at = Some(now);
                dispatch(MetricEvent::RequestRejected);
            }
            RequestState::Dropped | RequestState::Failed => {
                self.dropped_at = Some(now);
                dispatch(MetricEvent::RequestDropped);
            }
        }
    }
}
