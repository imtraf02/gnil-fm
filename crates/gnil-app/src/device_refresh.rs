use std::time::{Duration, Instant};

use gnil_fs::DeviceMonitor;

pub(crate) const DEVICE_MONITOR_INTERVAL: Duration = Duration::from_millis(750);
const MONITOR_RETRY_INTERVAL: Duration = Duration::from_secs(2);
const RECONCILE_INTERVAL: Duration = Duration::from_secs(15);

pub(crate) struct DeviceRefresh {
    monitor: Option<DeviceMonitor>,
    gate: RefreshGate,
    next_monitor_retry: Instant,
    next_reconcile: Instant,
}

impl DeviceRefresh {
    pub(crate) fn new() -> Self {
        let now = Instant::now();
        Self {
            monitor: DeviceMonitor::start().ok(),
            gate: RefreshGate::default(),
            next_monitor_retry: now + MONITOR_RETRY_INTERVAL,
            next_reconcile: now + RECONCILE_INTERVAL,
        }
    }

    pub(crate) const fn is_loading(&self) -> bool {
        self.gate.loading
    }

    pub(crate) fn request(&mut self) -> bool {
        self.gate.request()
    }

    pub(crate) fn finished(&mut self, succeeded: bool, now: Instant) -> bool {
        self.next_reconcile = now + reconcile_delay(succeeded);
        self.gate.finished()
    }

    pub(crate) fn tick(&mut self, now: Instant) -> bool {
        let mut refresh = self
            .monitor
            .as_ref()
            .is_some_and(DeviceMonitor::take_changed);

        if self.monitor.is_none() && now >= self.next_monitor_retry {
            self.monitor = DeviceMonitor::start().ok();
            self.next_monitor_retry = now + MONITOR_RETRY_INTERVAL;
            refresh |= self.monitor.is_some();
        }

        if now >= self.next_reconcile {
            self.next_reconcile = now + RECONCILE_INTERVAL;
            refresh = true;
        }

        refresh && self.gate.request()
    }
}

const fn reconcile_delay(succeeded: bool) -> Duration {
    if succeeded {
        RECONCILE_INTERVAL
    } else {
        MONITOR_RETRY_INTERVAL
    }
}

#[derive(Default)]
struct RefreshGate {
    loading: bool,
    pending: bool,
}

impl RefreshGate {
    fn request(&mut self) -> bool {
        if self.loading {
            self.pending = true;
            false
        } else {
            self.loading = true;
            true
        }
    }

    fn finished(&mut self) -> bool {
        if self.pending {
            self.pending = false;
            true
        } else {
            self.loading = false;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MONITOR_RETRY_INTERVAL, RECONCILE_INTERVAL, RefreshGate, reconcile_delay};

    #[test]
    fn coalesces_requests_while_refresh_is_running() {
        let mut gate = RefreshGate::default();
        assert!(gate.request());
        assert!(!gate.request());
        assert!(!gate.request());

        assert!(gate.finished());
        assert!(!gate.finished());
    }

    #[test]
    fn starts_a_new_refresh_after_the_previous_one_finishes() {
        let mut gate = RefreshGate::default();
        assert!(gate.request());
        assert!(!gate.finished());
        assert!(gate.request());
    }

    #[test]
    fn retries_failures_sooner_than_successful_reconciliation() {
        assert_eq!(reconcile_delay(false), MONITOR_RETRY_INTERVAL);
        assert_eq!(reconcile_delay(true), RECONCILE_INTERVAL);
    }
}
