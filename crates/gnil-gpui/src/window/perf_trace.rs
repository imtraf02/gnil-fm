use std::{
    env,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Default)]
struct TimingBucket {
    count: u64,
    total: Duration,
    max: Duration,
}

impl TimingBucket {
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

pub(super) struct WindowPerfTrace {
    enabled: bool,
    report_started: Instant,
    pending_input_since: Option<Instant>,
    dispatch: TimingBucket,
    draw: TimingBucket,
    submit: TimingBucket,
    frame: TimingBucket,
    input_to_submit_micros: Vec<u128>,
    missed_16ms: u64,
    missed_33ms: u64,
}

impl WindowPerfTrace {
    pub(super) fn from_env() -> Self {
        Self {
            enabled: env::var_os("GNIL_PERF_TRACE").is_some(),
            report_started: Instant::now(),
            pending_input_since: None,
            dispatch: TimingBucket::default(),
            draw: TimingBucket::default(),
            submit: TimingBucket::default(),
            frame: TimingBucket::default(),
            input_to_submit_micros: Vec::new(),
            missed_16ms: 0,
            missed_33ms: 0,
        }
    }

    #[inline]
    pub(super) fn start(&self) -> Option<Instant> {
        self.enabled.then(Instant::now)
    }

    pub(super) fn begin_input(&mut self) -> Option<Instant> {
        let started = self.start()?;
        self.pending_input_since.get_or_insert(started);
        Some(started)
    }

    pub(super) fn record_dispatch(&mut self, started: Option<Instant>, frame_requested: bool) {
        let Some(started) = started else {
            return;
        };
        self.dispatch.record(started.elapsed());
        if !frame_requested && self.pending_input_since == Some(started) {
            self.pending_input_since = None;
        }
        self.maybe_report();
    }

    pub(super) fn record_frame(
        &mut self,
        started: Option<Instant>,
        draw: Duration,
        submit: Duration,
    ) {
        let Some(started) = started else {
            return;
        };
        let elapsed = started.elapsed();
        self.draw.record(draw);
        self.submit.record(submit);
        self.frame.record(elapsed);
        if elapsed > Duration::from_micros(16_667) {
            self.missed_16ms = self.missed_16ms.saturating_add(1);
        }
        if elapsed > Duration::from_micros(33_333) {
            self.missed_33ms = self.missed_33ms.saturating_add(1);
        }
        if let Some(input_started) = self.pending_input_since.take() {
            self.input_to_submit_micros
                .push(input_started.elapsed().as_micros());
        }
        self.maybe_report();
    }

    fn maybe_report(&mut self) {
        if self.report_started.elapsed() < Duration::from_secs(1) {
            return;
        }

        self.input_to_submit_micros.sort_unstable();
        let input_p50 = percentile(&self.input_to_submit_micros, 50);
        let input_p95 = percentile(&self.input_to_submit_micros, 95);
        let input_p99 = percentile(&self.input_to_submit_micros, 99);
        let input_max = self.input_to_submit_micros.last().copied().unwrap_or(0);
        eprintln!(
            "gnil-frame frames:{} total:{}us/{}us draw:{}us/{}us submit:{}us/{}us \
             dispatch:{}@{}us/{}us input:{} p50:{}us p95:{}us p99:{}us max:{}us \
             missed:16.7ms={}/33.3ms={}",
            self.frame.count,
            self.frame.average_micros(),
            self.frame.max.as_micros(),
            self.draw.average_micros(),
            self.draw.max.as_micros(),
            self.submit.average_micros(),
            self.submit.max.as_micros(),
            self.dispatch.count,
            self.dispatch.average_micros(),
            self.dispatch.max.as_micros(),
            self.input_to_submit_micros.len(),
            input_p50,
            input_p95,
            input_p99,
            input_max,
            self.missed_16ms,
            self.missed_33ms,
        );
        self.reset_interval();
    }

    fn reset_interval(&mut self) {
        self.report_started = Instant::now();
        self.dispatch = TimingBucket::default();
        self.draw = TimingBucket::default();
        self.submit = TimingBucket::default();
        self.frame = TimingBucket::default();
        self.input_to_submit_micros.clear();
        self.missed_16ms = 0;
        self.missed_33ms = 0;
    }
}

fn percentile(samples: &[u128], percentile: usize) -> u128 {
    if samples.is_empty() {
        return 0;
    }
    let index = (samples.len() * percentile).div_ceil(100) - 1;
    samples[index]
}
