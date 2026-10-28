use std::future::Future;
use tokio::task::JoinHandle;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub fn tracker_tokio_spawn_with_cancel_token<F>(
    tracker: &TaskTracker,
    future: F,
    token: &CancellationToken,
    // NOTE: Consider adding a string ID to identify processes
) -> JoinHandle<()>
where
    F: Future + Send + 'static,
{
    let cloned_token = token.clone();
    tracker.spawn(async move {
        tokio::select! {
            _ = cloned_token.cancelled() => {
                println!("Cancelled");
                return;
            }
            _ = future => {
                return;
            }
        }
    })
}

pub fn tracker_tokio_spawn<F, T>(tracker: &TaskTracker, future: F) -> JoinHandle<(T, bool)>
where
    F: Future<Output = (T, bool)> + Send + 'static,
    T: Send + 'static,
{
    tracker.spawn(async move {
        tokio::select! {
            result = future => {
                result
            }
        }
    })
}
