#[derive(Clone, Copy, Debug, Default)]
struct RenderPerfBucket {
    count: u64,
    total: Duration,
    max: Duration,
}

impl RenderPerfBucket {
    fn record(&mut self, elapsed: Duration) {
        self.count = self.count.saturating_add(1);
        self.total = self.total.saturating_add(elapsed);
        self.max = self.max.max(elapsed);
    }

    fn average_micros(self) -> u128 {
        if self.count == 0 {
            0
        } else {
            self.total.as_micros() / u128::from(self.count)
        }
    }
}

struct PerfTrace {
    enabled: bool,
    report_started: Instant,
    root: RenderPerfBucket,
    surfaces: [RenderPerfBucket; FileManagerSurfaceKind::COUNT],
    pointer_received: u64,
    pointer_queued: u64,
    pointer_applied: u64,
    pointer_pending_since: Option<Instant>,
    pointer_latency_total: Duration,
    pointer_latency_max: Duration,
    pointer_latency_samples_micros: Vec<u128>,
}

impl PerfTrace {
    fn from_env() -> Self {
        Self {
            enabled: env::var_os("GNIL_PERF_TRACE").is_some(),
            report_started: Instant::now(),
            root: RenderPerfBucket::default(),
            surfaces: [RenderPerfBucket::default(); FileManagerSurfaceKind::COUNT],
            pointer_received: 0,
            pointer_queued: 0,
            pointer_applied: 0,
            pointer_pending_since: None,
            pointer_latency_total: Duration::ZERO,
            pointer_latency_max: Duration::ZERO,
            pointer_latency_samples_micros: Vec::new(),
        }
    }

    fn record_root(&mut self, elapsed: Duration) {
        if self.enabled {
            self.root.record(elapsed);
            self.maybe_report();
        }
    }

    fn record_surface(&mut self, kind: FileManagerSurfaceKind, elapsed: Duration) {
        if self.enabled {
            self.surfaces[kind.index()].record(elapsed);
            self.maybe_report();
        }
    }

    fn record_pointer_received(&mut self) {
        if self.enabled {
            self.pointer_received = self.pointer_received.saturating_add(1);
            self.maybe_report();
        }
    }

    fn record_pointer_queued(&mut self) {
        if self.enabled {
            self.pointer_queued = self.pointer_queued.saturating_add(1);
            self.pointer_pending_since = Some(Instant::now());
        }
    }

    fn record_pointer_applied(&mut self) {
        if !self.enabled {
            return;
        }
        self.pointer_applied = self.pointer_applied.saturating_add(1);
        if let Some(started) = self.pointer_pending_since.take() {
            let latency = started.elapsed();
            self.pointer_latency_total = self.pointer_latency_total.saturating_add(latency);
            self.pointer_latency_max = self.pointer_latency_max.max(latency);
            self.pointer_latency_samples_micros
                .push(latency.as_micros());
        }
        self.maybe_report();
    }

    fn maybe_report(&mut self) {
        if self.report_started.elapsed() < Duration::from_secs(1) {
            return;
        }
        let pointer_average = if self.pointer_applied == 0 {
            0
        } else {
            self.pointer_latency_total.as_micros() / u128::from(self.pointer_applied)
        };
        self.pointer_latency_samples_micros.sort_unstable();
        let pointer_p95 = if self.pointer_latency_samples_micros.is_empty() {
            0
        } else {
            let index = (self.pointer_latency_samples_micros.len() * 95).div_ceil(100) - 1;
            self.pointer_latency_samples_micros[index]
        };
        eprintln!(
            "gnil-perf root={} avg={}us max={}us surfaces=[{}] pointer=recv:{} queued:{} \
             applied:{} avg:{}us p95:{}us max:{}us",
            self.root.count,
            self.root.average_micros(),
            self.root.max.as_micros(),
            FileManagerSurfaceKind::ALL
                .iter()
                .map(|kind| {
                    let bucket = self.surfaces[kind.index()];
                    format!(
                        "{}:{}@{}us/{}us",
                        kind.name(),
                        bucket.count,
                        bucket.average_micros(),
                        bucket.max.as_micros()
                    )
                })
                .collect::<Vec<_>>()
                .join(","),
            self.pointer_received,
            self.pointer_queued,
            self.pointer_applied,
            pointer_average,
            pointer_p95,
            self.pointer_latency_max.as_micros(),
        );
        self.reset_interval();
    }

    fn reset_interval(&mut self) {
        self.report_started = Instant::now();
        self.root = RenderPerfBucket::default();
        self.surfaces = [RenderPerfBucket::default(); FileManagerSurfaceKind::COUNT];
        self.pointer_received = 0;
        self.pointer_queued = 0;
        self.pointer_applied = 0;
        self.pointer_latency_total = Duration::ZERO;
        self.pointer_latency_max = Duration::ZERO;
        self.pointer_latency_samples_micros.clear();
    }
}
