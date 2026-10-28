use std::{
    future::Future,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::time::Instant;

pub fn to_millis(secs: &u64) -> u64 {
    secs * 1000
}

/// Waits until the next interval and then executes
pub async fn fixed_interval_loop<T, F>(
    start_timestamp_ms: &u64,
    interval: &'static Duration,
    worker: &'static T,
    mut do_work: F,
    add_to_next_ms: &'static u64,
) where
    F: FnMut(&'static T, u64) + Send + 'static,
{
    let mut next_start = UNIX_EPOCH + Duration::from_millis(*start_timestamp_ms);
    let interval_millis = interval.as_millis();

    loop {
        // Schedule the next tick
        next_start += *interval;

        // How long until the next scheduled point?
        if wait_for(&next_start).await {
            let next_time = next_start + *interval;
            // Finally convert to UNIX‐epoch milliseconds
            let next_unix_ms = next_time
                .duration_since(UNIX_EPOCH)
                .expect("system time before UNIX EPOCH")
                .as_millis();

            let next_unix = (add_to_next_ms + next_unix_ms as u64 - interval_millis as u64) / 1000;
            do_work(worker, next_unix as u64);
        }
    }
}

/// Waits until the next interval and then executes (waits for action to finish)
pub async fn fixed_interval_loop_async<T, F, Fut>(
    start_timestamp_ms: &u64,
    interval: &'static Duration,
    worker: &'static T,
    mut do_work: F,
) where
    F: FnMut(&'static T, u64) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let mut next_start = UNIX_EPOCH + Duration::from_millis(*start_timestamp_ms);

    loop {
        // Schedule the next tick
        next_start += *interval;

        // How long until the next scheduled point?
        if wait_for(&next_start).await {
            let next_time = next_start + *interval;
            // Finally convert to UNIX‐epoch milliseconds
            let next_unix_ms = next_time
                .duration_since(UNIX_EPOCH)
                .expect("system time before UNIX EPOCH")
                .as_millis();

            // This is being rounded down if start_timestamp_ms does not start at specific second.
            let next_unix = (next_unix_ms as u64) / 1000;
            do_work(worker, next_unix).await;
        }
    }
}

/// Waits until `timestamp` (in ms since UNIX epoch).
/// Returns `true` if it waited, or `false` if the target time had already passed.
pub async fn wait_for_timestamp_ms(timestamp: &u64) -> bool {
    let timestamp = UNIX_EPOCH + Duration::from_millis(*timestamp);

    let duration_until = match get_duration_until_ms(&timestamp) {
        Ok(value) => value,
        Err(value) => return value, // false
    };

    // Sleep until that moment
    tokio::time::sleep_until(Instant::now() + duration_until).await;

    return true;
}

/// Waits until `timestamp` (in ms since UNIX epoch).
/// Returns `true` if it waited, or `false` if the target time had already passed.
async fn wait_for(timestamp: &SystemTime) -> bool {
    let duration_until = match get_duration_until_ms(timestamp) {
        Ok(value) => value,
        Err(value) => return value, // false
    };

    // Sleep until that moment
    tokio::time::sleep_until(Instant::now() + duration_until).await;

    return true;
}

fn get_duration_until_ms(timestamp: &SystemTime) -> Result<Duration, bool> {
    let now = SystemTime::now();

    match timestamp.duration_since(now) {
        Ok(dur) => Ok(dur),
        Err(_) => Err(false), // Time already passed
    }
}
