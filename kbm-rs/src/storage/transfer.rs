use serde::Serialize;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
#[derive(Clone, Serialize, schemars::JsonSchema)]
pub struct Progress {
    pub bytes_per_second: Option<f64>,
    pub remaining_seconds: Option<f64>,
    pub elapsed_seconds: f64,
}
#[derive(Clone)]
pub(super) struct Meter {
    started: Instant,
    updated: Instant,
    done: u64,
    total: u64,
    samples: VecDeque<(Instant, u64)>,
}
impl Meter {
    pub fn new(total: u64, now: Instant) -> Self {
        Self {
            started: now,
            updated: now,
            done: 0,
            total,
            samples: VecDeque::from([(now, 0)]),
        }
    }
    pub fn advance(&mut self, done: u64, now: Instant) {
        assert!(done >= self.done && done <= self.total);
        if done > self.done {
            self.updated = now;
        }
        self.done = done;
        if now.duration_since(self.samples.back().unwrap().0) >= Duration::from_millis(250) {
            self.samples.push_back((now, done));
        }
        self.trim(now);
    }
    fn trim(&mut self, now: Instant) {
        while self.samples.len() > 1
            && now.duration_since(self.samples[1].0) >= Duration::from_secs(5)
        {
            self.samples.pop_front();
        }
        while self.samples.len() > 24 {
            self.samples.pop_front();
        }
    }
    pub fn progress(&mut self, now: Instant) -> Progress {
        self.trim(now);
        let elapsed = now.duration_since(self.started).as_secs_f64();
        if elapsed < 1.0 && self.done < self.total {
            return Progress {
                bytes_per_second: None,
                remaining_seconds: None,
                elapsed_seconds: elapsed,
            };
        }
        let (base, done) = *self.samples.front().unwrap();
        let seconds = now.duration_since(base).as_secs_f64().min(5.0);
        let rate = if now.duration_since(self.updated) >= Duration::from_secs(5) || seconds <= 0.0 {
            0.0
        } else {
            (self.done - done) as f64 / seconds
        };
        Progress {
            bytes_per_second: Some(rate),
            remaining_seconds: if self.done >= self.total {
                Some(0.0)
            } else if rate > 0.0 {
                Some((self.total - self.done) as f64 / rate)
            } else {
                None
            },
            elapsed_seconds: elapsed,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rate_is_confirmed_payload_only_and_stalls_expire_eta() {
        let t = Instant::now();
        let mut m = Meter::new(10000, t);
        assert!(m
            .progress(t + Duration::from_millis(500))
            .bytes_per_second
            .is_none());
        m.advance(1000, t + Duration::from_secs(1));
        let p = m.progress(t + Duration::from_secs(1));
        assert_eq!(p.bytes_per_second, Some(1000.0));
        assert_eq!(p.remaining_seconds, Some(9.0));
        let p = m.progress(t + Duration::from_secs(6));
        assert_eq!(p.bytes_per_second, Some(0.0));
        assert_eq!(p.remaining_seconds, None);
        m.advance(10000, t + Duration::from_secs(7));
        assert_eq!(
            m.progress(t + Duration::from_secs(7)).remaining_seconds,
            Some(0.0)
        );
    }
}
