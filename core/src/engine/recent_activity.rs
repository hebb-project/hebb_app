//! Bounded ring buffer of recent engine spike events ([C3] / hebb_app#26).
//!
//! Background: spikes are broadcast on a tokio `broadcast` channel
//! (`SimHandle::spikes`). That channel only delivers to *currently
//! subscribed* receivers — late subscribers see whatever the channel still
//! holds in its lag window, and a tool invoked without a live WS subscriber
//! (the agent harness case, [B2] / hebb_app#15) gets nothing. The agent
//! needs a way to ask "what happened in the last N spikes?" without keeping
//! a WS open.
//!
//! This buffer is held by the engine actor task and updated on every tick
//! that produces events. It is intentionally:
//!
//! - **Bounded**: a fixed `capacity` of `SpikeEvent` records; oldest are
//!   evicted as new ones arrive. Memory is `O(capacity * size_of::<SpikeEvent>)`
//!   — at the default 8 192 events that's ~196 KB.
//! - **Single-owner**: lives inside the engine task, no `Mutex` / `RwLock`.
//!   Reads go through an [`EngineCommand`] one-shot reply, the same shape
//!   every other introspection call uses.
//! - **Time-aware**: read API takes both a `limit` and an optional
//!   `since_t_ms` so callers can ask either "the last N" or "everything
//!   since clock t" without re-implementing the filter at every call site.
//!
//! Voltage and arbitrary domain-event history are deliberately out of scope
//! here. [B2] also wants `recent_voltages` and `recent_events` tools — those
//! are sampled (voltage) or domain-specific (events) and warrant their own
//! buffer type. Keeping this one spikes-only makes the eviction semantics
//! and the memory bound easy to reason about; the other history streams can
//! be added as sibling buffers in this module without churning the public
//! API of the engine actor.

use std::collections::VecDeque;

pub use hebb::engine::events::SpikeEvent;

/// Default capacity (in events, not frames) for the recent-activity buffer.
///
/// Sized so a moderately active LIF net (~1 kHz aggregate firing rate
/// across the network) holds roughly the last 8 seconds of spikes. Easy to
/// hold in memory, large enough that an agent asking for "the last hundred
/// spikes" essentially never reaches into stale state.
pub const RECENT_ACTIVITY_DEFAULT_CAPACITY: usize = 8_192;

/// Bounded ring buffer of recent [`SpikeEvent`]s held by the engine actor.
///
/// Eviction is oldest-first. Reads return events in *chronological* order —
/// the natural order they were pushed in — so callers don't have to think
/// about the internal ring representation.
#[derive(Debug)]
pub struct RecentActivityBuffer {
    spikes: VecDeque<SpikeEvent>,
    capacity: usize,
}

impl RecentActivityBuffer {
    /// Construct a buffer with the given capacity. A capacity of `0` is
    /// treated as `1` so the buffer is always valid to push into; callers
    /// almost certainly meant "the default" and we'd rather not panic them
    /// out of a misconfigured env var.
    pub fn with_capacity(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            spikes: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Append one spike event, evicting the oldest if the buffer is full.
    pub fn push_spike(&mut self, event: SpikeEvent) {
        if self.spikes.len() == self.capacity {
            self.spikes.pop_front();
        }
        self.spikes.push_back(event);
    }

    /// Append a batch of spike events in order. The buffer's eviction
    /// semantics still apply, so a `batch.len() > capacity` call effectively
    /// keeps only the tail.
    pub fn extend_spikes<I: IntoIterator<Item = SpikeEvent>>(&mut self, events: I) {
        for ev in events {
            self.push_spike(ev);
        }
    }

    /// Snapshot the last `limit` spikes (or fewer if the buffer is shorter),
    /// optionally filtered to events with `t_ms >= since_t_ms`. Returned in
    /// chronological order.
    ///
    /// A `limit` of `0` returns an empty vector — useful as a cheap "buffer
    /// alive?" probe.
    pub fn recent_spikes(&self, limit: usize, since_t_ms: Option<f64>) -> Vec<SpikeEvent> {
        if limit == 0 || self.spikes.is_empty() {
            return Vec::new();
        }
        let iter = self.spikes.iter().filter(|ev| match since_t_ms {
            Some(t) => ev.t_ms >= t,
            None => true,
        });
        // Take from the tail: we want the *most recent* `limit` matches.
        // Collect first to get the count, then slice the tail. The buffer
        // is bounded, so this allocation is bounded too.
        let matched: Vec<&SpikeEvent> = iter.collect();
        let start = matched.len().saturating_sub(limit);
        matched[start..].iter().map(|&ev| ev.clone()).collect()
    }
}

impl Default for RecentActivityBuffer {
    fn default() -> Self {
        Self::with_capacity(RECENT_ACTIVITY_DEFAULT_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn ev(t_ms: f64) -> SpikeEvent {
        SpikeEvent { node_id: Uuid::new_v4(), t_ms }
    }

    #[test]
    fn capacity_is_enforced_on_push() {
        let mut buf = RecentActivityBuffer::with_capacity(3);
        for t in 0..5 {
            buf.push_spike(ev(t as f64));
        }
        assert_eq!(buf.spikes.len(), 3);
        // Oldest two were evicted.
        let all = buf.recent_spikes(10, None);
        let times: Vec<f64> = all.iter().map(|e| e.t_ms).collect();
        assert_eq!(times, vec![2.0, 3.0, 4.0]);
    }

    #[test]
    fn zero_capacity_clamped_to_one() {
        let mut buf = RecentActivityBuffer::with_capacity(0);
        buf.push_spike(ev(1.0));
        buf.push_spike(ev(2.0));
        assert_eq!(buf.spikes.len(), 1);
        assert_eq!(buf.recent_spikes(10, None)[0].t_ms, 2.0);
    }

    #[test]
    fn recent_spikes_clamps_to_buffer_size() {
        let mut buf = RecentActivityBuffer::with_capacity(10);
        buf.push_spike(ev(1.0));
        buf.push_spike(ev(2.0));
        let out = buf.recent_spikes(100, None);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn recent_spikes_filters_by_since() {
        let mut buf = RecentActivityBuffer::with_capacity(10);
        buf.extend_spikes((0..5).map(|t| ev(t as f64)));
        let out = buf.recent_spikes(10, Some(3.0));
        let times: Vec<f64> = out.iter().map(|e| e.t_ms).collect();
        assert_eq!(times, vec![3.0, 4.0]);
    }

    #[test]
    fn recent_spikes_limit_takes_tail() {
        let mut buf = RecentActivityBuffer::with_capacity(10);
        buf.extend_spikes((0..5).map(|t| ev(t as f64)));
        let out = buf.recent_spikes(2, None);
        let times: Vec<f64> = out.iter().map(|e| e.t_ms).collect();
        assert_eq!(times, vec![3.0, 4.0]);
    }

    #[test]
    fn recent_spikes_limit_zero_returns_empty() {
        let mut buf = RecentActivityBuffer::with_capacity(10);
        buf.push_spike(ev(1.0));
        assert!(buf.recent_spikes(0, None).is_empty());
    }

    #[test]
    fn since_with_no_matches_returns_empty() {
        let mut buf = RecentActivityBuffer::with_capacity(10);
        buf.extend_spikes((0..3).map(|t| ev(t as f64)));
        assert!(buf.recent_spikes(10, Some(100.0)).is_empty());
    }
}
