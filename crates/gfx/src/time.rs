//! Time-related utilities, like storage of delta time and calculation of average frame rate.

use std::collections::VecDeque;
use std::iter::Sum;
use std::time::{Duration, Instant};

/// The current time state of all parts of the engine.
#[derive(Debug, Clone)]
pub struct Time {
    /// The current instant in time.
    ///
    /// Prefer to use this over Instant::now() in order to use the same value during every tick.
    pub now: Instant,
    /// The speed at which the render-thread runs.
    pub render: UpdateRate,
    /// The speed at which the simulation-thread runs **as seen from the render thread's perspective**,
    /// since this is based on data sent from the simulation thread it is _not_ up-to-date.
    pub simulation: UpdateRate,
}

impl Time {
    /// Creates a new `Time` instance with a set average-buffer size.
    ///
    /// The `average_buffer_size` is not the _average size_ of some buffer, it's the _size_
    /// of _the average-buffer_ - how many samples are used when calculating the average frame rate.
    pub fn new(now: Instant, average_buffer_size: usize) -> Self {
        Self {
            now,
            render: UpdateRate::new(now, average_buffer_size),
            simulation: UpdateRate::new(now, average_buffer_size),
        }
    }
}

/// How often something is updated, and in which interval it does so.
#[derive(Debug, Clone)]
pub struct UpdateRate {
    last_update: Instant,
    /// How much time has passed between the last update and the current.
    pub delta_time: Duration,
    /// Stores the time samples used for averages.
    ///
    /// New samples lie in the back (`push_back`) and old samples lie in the front (`pop_front`).
    average_delta_time_buffer: VecDeque<Duration>,
}

impl UpdateRate {
    /// Creates a new instance of `UpdateRate`.
    ///
    /// The `average_buffer_size` is not the _average size_ of some buffer, it's the _size_
    /// of _the average-buffer_ - how many samples are used when calculating the average frame rate.
    pub fn new(last_update: Instant, average_buffer_size: usize) -> Self {
        Self {
            last_update,
            delta_time: Duration::ZERO,
            average_delta_time_buffer: VecDeque::with_capacity(average_buffer_size),
        }
    }

    /// Informs about a new moment in time, e.g. when a new tick has begun.
    pub fn update_time(&mut self, now: Instant) {
        self.delta_time = now - self.last_update;
        self.add_delta_time_sample(self.delta_time);
        self.last_update = now;
    }

    /// Informs about new delta_time-samples describing how fast something is progressing.
    ///
    /// Accepts an arbitrary number of samples, but discards the oldest ones if the total count
    /// exceeds `average_buffer_size`.
    pub fn update_from_delta_samples<Iter>(&mut self, iter: Iter)
    where
        Iter: IntoIterator<Item = Duration>,
    {
        iter.into_iter()
            .for_each(|sample| self.add_delta_time_sample(sample));
    }

    fn add_delta_time_sample(&mut self, sample: Duration) {
        if self.average_delta_time_buffer.len() == self.average_delta_time_buffer.capacity() {
            self.average_delta_time_buffer.pop_front(); // Discard the oldest sample.
        }
        self.average_delta_time_buffer.push_back(sample);
    }

    /// Calculates the average number of frames per second, based on a number of time samples
    /// gathered previously.
    pub fn average_fps(&self) -> f32 {
        let duration_sample_count = self.average_delta_time_buffer.len() as u32;
        let average_duration = Duration::sum(self.average_delta_time_buffer.iter())
            / std::cmp::max(1, duration_sample_count);
        1.0 / average_duration.as_secs_f32()
    }

    fn average_sample_count(&self) -> u32 {
        self.average_delta_time_buffer.len() as u32
    }
}

impl std::fmt::Display for UpdateRate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let fps = self.average_fps();
        let duration_sample_count = self.average_sample_count();
        write!(f, "{fps} fps (average over {duration_sample_count} ticks)")
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Add;

    use approx::assert_ulps_eq;
    use proptest::prelude::*;
    use test_strategy::proptest;

    use super::*;

    prop_compose! {
        fn arb_nonnegative_duration(max_seconds: f32)
                                   (seconds in 0_f32..max_seconds)
                                   -> Duration {
            Duration::from_secs_f32(seconds)
        }
    }
    prop_compose! {
        fn arb_buffersize_and_sample_count(max_buffer_size: usize, min_sample_count: usize, max_sample_count: usize)
                                          (buffer_size in 1_usize..=max_buffer_size)
                                          (sample in min_sample_count..=max_sample_count, buffer_size in Just(buffer_size))
                                          -> (usize, u64) {
           (buffer_size, sample as u64)
       }
    }

    #[proptest]
    fn delta_time_matches_time_between_updates(
        #[strategy(arb_nonnegative_duration(10.0))] time_passed: Duration,
    ) {
        let beginning = Instant::now();
        let later = beginning.add(time_passed);
        let mut update_rate = UpdateRate::new(beginning, 1);

        assert_eq!(update_rate.delta_time, Duration::ZERO);
        update_rate.update_time(later);
        assert_eq!(update_rate.delta_time, later.duration_since(beginning));
    }

    #[proptest]
    fn update_time_vacates_old_samples_from_buffer(
        #[strategy(arb_buffersize_and_sample_count(5, 1, 10))] tuple: (usize, u64),
    ) {
        let (buffer_size, new_sample_count) = tuple;
        let beginning = Instant::now();
        let mut update_rate = UpdateRate::new(beginning, buffer_size);

        for sample in 1..=new_sample_count {
            let mut later = beginning + Duration::from_secs(sample);
            if sample == new_sample_count {
                // To distinguish the latest sample from the rest, make it 1 second later so the
                // delta between it and the previous is 2 rather than 1.
                later += Duration::from_secs(1);
            }
            update_rate.update_time(later);
        }

        assert_eq!(
            update_rate
                .average_delta_time_buffer
                .pop_back()
                .unwrap()
                .as_secs(),
            2
        )
    }

    #[proptest]
    fn update_from_samples_vacates_overflowing_samples(
        #[strategy(arb_buffersize_and_sample_count(5, 6, 10))] tuple: (usize, u64),
    ) {
        let (buffer_size, new_sample_count) = tuple;
        let beginning = Instant::now();
        let mut update_rate = UpdateRate::new(beginning, buffer_size);

        let samples = (0..new_sample_count).map(Duration::from_secs);
        update_rate.update_from_delta_samples(samples);

        assert_eq!(
            update_rate
                .average_delta_time_buffer
                .pop_back()
                .unwrap()
                .as_secs(),
            new_sample_count - 1
        )
    }

    #[proptest]
    fn frame_rate_depends_on_time_between_updates(#[strategy(1_f32..60_f32)] fps: f32) {
        let beginning = Instant::now();
        let time_passed = Duration::from_secs_f32(1.0 / fps);
        let later = beginning.add(time_passed);
        let mut update_rate = UpdateRate::new(beginning, 1);

        update_rate.update_time(later);

        assert_ulps_eq!(update_rate.average_fps(), fps);
    }
}
