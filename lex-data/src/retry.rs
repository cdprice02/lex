use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) const ATTEMPTS: u32 = 3;
const BASE_BACKOFF: Duration = Duration::from_secs(1);

/// Why one attempt failed, which decides whether another is worth making.
#[derive(Debug)]
pub(crate) enum Failure {
    /// A fault a repeat can plausibly clear: connect error, timeout, 5xx, a dropped body.
    Transient(anyhow::Error),
    /// A fault a repeat cannot clear, such as a 404.
    Permanent(anyhow::Error),
}

/// Runs `op` up to [`ATTEMPTS`] times, sleeping an exponentially growing, jittered delay between
/// transient failures. `sleep` is injected so tests do not wait.
pub(crate) fn with_retry<T>(
    mut op: impl FnMut() -> Result<T, Failure>,
    mut sleep: impl FnMut(Duration),
) -> anyhow::Result<T> {
    for attempt in 1.. {
        match op() {
            Ok(value) => return Ok(value),
            Err(Failure::Permanent(e)) => return Err(e),
            Err(Failure::Transient(e)) if attempt >= ATTEMPTS => return Err(e),
            Err(Failure::Transient(e)) => {
                let delay = backoff(attempt, clock_jitter());
                log::warn!("attempt {attempt}/{ATTEMPTS} failed: {e:#}; retrying in {delay:?}");
                sleep(delay);
            }
        }
    }
    unreachable!("the loop returns on success or on the final attempt")
}

/// `BASE_BACKOFF * 2^(attempt - 1)`, plus up to half of that again as `jitter` (in `0.0..1.0`),
/// so concurrent shard workers that failed together do not retry together.
fn backoff(attempt: u32, jitter: f64) -> Duration {
    let base = BASE_BACKOFF * 2u32.pow(attempt - 1);
    base + base.mul_f64(jitter * 0.5)
}

/// The sub-second part of the wall clock, as a fraction: unpredictable enough to spread retries
/// without a random-number dependency.
fn clock_jitter() -> f64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    f64::from(nanos) / 1e9
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transient<T>(msg: &str) -> Result<T, Failure> {
        Err(Failure::Transient(anyhow::anyhow!(msg.to_string())))
    }

    #[test]
    fn succeeds_after_transient_failures() {
        let mut calls = 0;
        let mut sleeps = Vec::new();
        let value = with_retry(
            || {
                calls += 1;
                if calls < 3 {
                    transient("flaky")
                } else {
                    Ok(calls)
                }
            },
            |d| sleeps.push(d),
        )
        .unwrap();
        assert_eq!(value, 3);
        assert_eq!(sleeps.len(), 2);
        assert!(sleeps[1] > sleeps[0], "{sleeps:?}");
    }

    #[test]
    fn gives_up_after_the_attempt_limit() {
        let mut calls = 0;
        let err = with_retry::<()>(
            || {
                calls += 1;
                transient("down")
            },
            |_| {},
        )
        .unwrap_err();
        assert_eq!(calls, ATTEMPTS);
        assert_eq!(err.to_string(), "down");
    }

    #[test]
    fn permanent_failure_is_not_retried() {
        let mut calls = 0;
        let mut slept = false;
        let err = with_retry::<()>(
            || {
                calls += 1;
                Err(Failure::Permanent(anyhow::anyhow!("404")))
            },
            |_| slept = true,
        )
        .unwrap_err();
        assert_eq!(calls, 1);
        assert!(!slept);
        assert_eq!(err.to_string(), "404");
    }

    #[test]
    fn backoff_doubles_and_jitter_stays_within_half_a_step() {
        assert_eq!(backoff(1, 0.0), Duration::from_secs(1));
        assert_eq!(backoff(2, 0.0), Duration::from_secs(2));
        assert_eq!(backoff(1, 1.0), Duration::from_millis(1500));
        assert_eq!(backoff(2, 1.0), Duration::from_secs(3));
    }
}
