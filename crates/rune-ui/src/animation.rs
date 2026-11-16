use std::{
    marker::PhantomData,
    time::{Duration, Instant},
};
#[derive(Debug, Clone)]
pub(crate) struct Animation<T> {
    target: bool,
    from: f32,
    velocity: f32,
    at: Instant,
    duration: Duration,
    marker: PhantomData<T>,
}
impl Animation<bool> {
    pub fn new(value: bool) -> Self {
        Self {
            target: value,
            from: if value { 1. } else { 0. },
            velocity: 0.,
            at: Instant::now(),
            duration: Duration::from_millis(200),
            marker: PhantomData,
        }
    }
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }
    pub fn value(&self) -> bool {
        self.target
    }
    fn sample(&self, now: Instant) -> (f32, f32) {
        let target = if self.target { 1. } else { 0. };
        let elapsed = now.saturating_duration_since(self.at).as_secs_f32();
        let response = self.duration.as_secs_f32();
        if response == 0. || elapsed >= response * 2. {
            return (target, 0.);
        }
        let frequency = 8. / response;
        let displacement = self.from - target;
        let coefficient = self.velocity + frequency * displacement;
        let decay = (-frequency * elapsed).exp();
        let position = target + (displacement + coefficient * elapsed) * decay;
        let velocity = (self.velocity - frequency * coefficient * elapsed) * decay;
        (position, velocity)
    }
    fn progress(&self, now: Instant) -> f32 {
        self.sample(now).0
    }
    pub fn go_mut(&mut self, value: bool, now: Instant) {
        if self.target == value {
            return;
        }
        (self.from, self.velocity) = self.sample(now);
        self.target = value;
        self.at = now;
    }
    pub fn go(mut self, value: bool, now: Instant) -> Self {
        self.go_mut(value, now);
        self
    }
    pub fn is_animating(&self, now: Instant) -> bool {
        let (position, velocity) = self.sample(now);
        (position - if self.target { 1. } else { 0. }).abs() > 0.0001 || velocity.abs() > 0.001
    }

    pub fn interpolate(&self, from: f32, to: f32, now: Instant) -> f32 {
        from + (to - from) * self.progress(now)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reversal_preserves_position_and_velocity() {
        let now = Instant::now();
        let mut animation = Animation::new(false)
            .duration(Duration::from_millis(300))
            .go(true, now);
        let reversal = now + Duration::from_millis(60);
        let step = Duration::from_micros(100);
        let position = animation.progress(reversal);
        let incoming = (position - animation.progress(reversal - step)) / step.as_secs_f32();
        animation.go_mut(false, reversal);
        assert!((animation.progress(reversal) - position).abs() < 0.00001);
        let outgoing = (animation.progress(reversal + step) - position) / step.as_secs_f32();
        assert!(
            (outgoing - incoming).abs() < 0.05,
            "{incoming} -> {outgoing}"
        );
        assert_eq!(animation.progress(reversal + Duration::from_secs(2)), 0.);
        assert!(!animation.is_animating(reversal + Duration::from_secs(2)));
    }
    #[test]
    fn repeated_target_does_not_restart_motion() {
        let now = Instant::now();
        let mut animation = Animation::new(false).go(true, now);
        animation.go_mut(true, now + Duration::from_millis(50));
        assert_eq!(animation.at, now);
    }
    #[test]
    fn critically_damped_entry_has_no_overshoot_and_settles() {
        let now = Instant::now();
        let animation = Animation::new(false).go(true, now);
        let mut previous = 0.;
        for milliseconds in 0..=500 {
            let value = animation.progress(now + Duration::from_millis(milliseconds));
            assert!((previous..=1.).contains(&value));
            previous = value;
        }
        assert_eq!(previous, 1.);
        assert!(!animation.is_animating(now + Duration::from_secs(1)));
    }
    #[test]
    fn disabled_animation_settles_immediately() {
        let now = Instant::now();
        let animation = Animation::new(false).duration(Duration::ZERO).go(true, now);
        assert_eq!(animation.interpolate(10., 20., now), 20.);
        assert!(!animation.is_animating(now));
    }
}
