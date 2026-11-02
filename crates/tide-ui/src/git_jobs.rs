//! One bounded FIFO for Git work. Mutations and subsequent reads retain intent order.
use crate::app::AppEvent;
use crate::jobs::{Jobs, Tracked};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
type Work = Box<dyn FnOnce(&Arc<AtomicBool>) -> AppEvent + Send>;
struct Job {
    run: Work,
    failed: Box<dyn FnOnce() -> AppEvent + Send>,
    _tracked: Tracked,
}
pub struct GitJobs {
    jobs: Jobs,
    sender: Option<tokio::sync::mpsc::Sender<Job>>,
    cancel: Arc<AtomicBool>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl GitJobs {
    pub fn new(jobs: &Jobs, events: tokio::sync::mpsc::Sender<AppEvent>) -> Self {
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<Job>(64);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let task = jobs.spawn_daemon(async move {
            let mut sequence = 0_u64;
            while let Some(job) = receiver.recv().await {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                let flag = stop.clone();
                sequence += 1;
                let started = std::time::Instant::now();
                tracing::debug!(job = sequence, "Git work started");
                let Job {
                    run,
                    failed,
                    _tracked,
                } = job;
                let event = match tokio::task::spawn_blocking(move || run(&flag)).await {
                    Ok(event) => event,
                    Err(error) => {
                        tracing::error!(
                            job = sequence,
                            panicked = error.is_panic(),
                            "Git worker failed"
                        );
                        failed()
                    }
                };
                tracing::debug!(
                    job = sequence,
                    elapsed_ms = started.elapsed().as_millis(),
                    "Git work finished"
                );
                loop {
                    if stop.load(Ordering::Acquire) {
                        return;
                    }
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(50),
                        events.reserve(),
                    )
                    .await
                    {
                        Ok(Ok(permit)) => {
                            permit.send(event);
                            break;
                        }
                        Ok(Err(_)) => return,
                        Err(_) => {}
                    }
                }
            }
        });
        Self {
            jobs: jobs.clone(),
            sender: Some(sender),
            cancel,
            task: Some(task),
        }
    }
    pub fn submit(&self, job: impl FnOnce(&Arc<AtomicBool>) -> AppEvent + Send + 'static) -> bool {
        self.submit_with_failure(job, || {
            AppEvent::Error("Git worker failed; retry the action".into())
        })
    }
    pub fn submit_with_failure(
        &self,
        job: impl FnOnce(&Arc<AtomicBool>) -> AppEvent + Send + 'static,
        failed: impl FnOnce() -> AppEvent + Send + 'static,
    ) -> bool {
        self.sender.as_ref().is_some_and(|sender| {
            sender
                .try_send(Job {
                    run: Box::new(job),
                    failed: Box::new(failed),
                    _tracked: self.jobs.track(),
                })
                .is_ok()
        })
    }
    pub async fn shutdown(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.sender.take();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for GitJobs {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn failed_worker_delivers_its_failure_and_keeps_fifo_moving() {
        let (sender, mut events) = tokio::sync::mpsc::channel(4);
        let mut jobs = GitJobs::new(
            &crate::jobs::Jobs::runtime(tokio::runtime::Handle::current()),
            sender,
        );
        assert!(jobs.submit_with_failure(
            |_| panic!("fixture failure"),
            || AppEvent::Error("first failed".into())
        ));
        assert!(jobs.submit(|_| AppEvent::Error("second finished".into())));
        for expected in ["first failed", "second finished"] {
            // Generous: a loaded CI runner can spend seconds printing the panic's backtrace.
            let event = tokio::time::timeout(std::time::Duration::from_secs(10), events.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(matches!(event, AppEvent::Error(message) if message == expected));
        }
        jobs.shutdown().await;
    }
}
