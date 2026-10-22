//! A job queue shared by the tools: how many jobs are accepted and run at once, one active job
//! per client address, live status for job pages, and cleanup of abandoned and finished jobs.
//!
//! A job starts as a reservation ([`Queue::reserve`]) so the browser learns whether there is
//! room before it uploads anything. The upload then fills the reservation ([`Upload::submit`])
//! and the job waits for one of the running slots, which are handed out in upload order.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::{Semaphore, watch};
use tokio::time::Instant;

/// How often [`Queue::spawn_sweeper`] looks for jobs to cancel or forget.
const SWEEP_INTERVAL: Duration = Duration::from_secs(5);

/// How many jobs a queue takes and how long it keeps them.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Jobs accepted at once: reserved, waiting and running ones together.
    pub jobs: usize,
    /// Jobs running at once.
    pub running: usize,
    /// How long a reservation may wait for its upload to arrive.
    pub upload_start: Duration,
    /// How long an unfinished job may go without an open job page before it is cancelled.
    pub unwatched: Duration,
    /// How long a finished job and its result are kept.
    pub keep_finished: Duration,
}

/// Identifies a job. Long and random, because a job's link is the only access to its result.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JobId(String);

impl JobId {
    fn random() -> Self {
        Self(format!("{:032x}", rand::random::<u128>()))
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for JobId {
    type Err = InvalidJobId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let valid = s.len() == 32 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if valid {
            Ok(Self(s.to_owned()))
        } else {
            Err(InvalidJobId)
        }
    }
}

/// A string that is not a job id.
#[derive(Debug)]
pub struct InvalidJobId;

impl fmt::Display for InvalidJobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("not a job id")
    }
}

impl std::error::Error for InvalidJobId {}

/// Where a job is in its life.
#[derive(Debug)]
pub enum Status<R> {
    /// Reserved; the upload has not arrived yet.
    Reserved,
    /// Waiting for a running slot.
    Waiting {
        /// 1 for the next job to run.
        position: usize,
    },
    /// Running.
    Running {
        /// Estimated progress, 0 to 100.
        percent: u8,
    },
    /// Finished with a result.
    Done(Arc<R>),
    /// Finished without a result. The tool logged why.
    Failed,
    /// Stopped before it finished.
    Cancelled(CancelReason),
}

impl<R> Status<R> {
    /// Whether the job has ended, with or without a result.
    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Done(_) | Self::Failed | Self::Cancelled(_))
    }
}

// Derived `Clone` would require `R: Clone`; results are shared through an `Arc`.
impl<R> Clone for Status<R> {
    fn clone(&self) -> Self {
        match self {
            Self::Reserved => Self::Reserved,
            Self::Waiting { position } => Self::Waiting {
                position: *position,
            },
            Self::Running { percent } => Self::Running { percent: *percent },
            Self::Done(result) => Self::Done(Arc::clone(result)),
            Self::Failed => Self::Failed,
            Self::Cancelled(reason) => Self::Cancelled(*reason),
        }
    }
}

/// Why a job was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    /// Nobody had its page open for too long.
    Unwatched,
    /// The server is shutting down.
    Shutdown,
}

/// Why the queue refused a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    /// The server is shutting down.
    Closed,
    /// The queue holds as many jobs as it accepts.
    Full,
    /// This client already has an unfinished job.
    ActiveJob,
    /// No reservation with this id is waiting for an upload from this client.
    NoReservation,
}

/// A queue of jobs producing results of type `R`. Clones share the queue.
pub struct Queue<R> {
    shared: Arc<Shared<R>>,
}

impl<R> Clone for Queue<R> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

struct Shared<R> {
    limits: Limits,
    running: Semaphore,
    state: Mutex<State<R>>,
}

struct State<R> {
    jobs: HashMap<JobId, Job<R>>,
    /// Waiting jobs, next to run first.
    waiting: VecDeque<JobId>,
    /// Set on shutdown: nothing new is accepted.
    closed: bool,
}

struct Job<R> {
    client: Option<IpAddr>,
    status: watch::Sender<Status<R>>,
    /// Open job pages.
    watchers: usize,
    /// An upload request is filling this reservation.
    uploading: bool,
    /// When the job was reserved, submitted, last left without a page, or finished, whichever
    /// is latest. The cleanup rules measure from here.
    idle_since: Instant,
}

impl<R: Send + Sync + 'static> Queue<R> {
    pub fn new(limits: Limits) -> Self {
        Self {
            shared: Arc::new(Shared {
                running: Semaphore::new(limits.running),
                limits,
                state: Mutex::new(State {
                    jobs: HashMap::new(),
                    waiting: VecDeque::new(),
                    closed: false,
                }),
            }),
        }
    }

    /// Whether `client` could reserve a job right now.
    pub fn availability(&self, client: Option<IpAddr>) -> Result<(), Rejection> {
        self.lock().admits(client, self.shared.limits.jobs)
    }

    /// Reserves a job for `client`, to be filled by an upload.
    pub fn reserve(&self, client: Option<IpAddr>) -> Result<JobId, Rejection> {
        let mut state = self.lock();
        state.admits(client, self.shared.limits.jobs)?;
        let id = JobId::random();
        state.jobs.insert(
            id.clone(),
            Job {
                client,
                status: watch::Sender::new(Status::Reserved),
                watchers: 0,
                uploading: false,
                idle_since: Instant::now(),
            },
        );
        Ok(id)
    }

    /// Starts filling the reservation `id` made by `client`. Dropping the returned upload
    /// without submitting it releases the reservation.
    pub fn begin_upload(&self, id: &JobId, client: Option<IpAddr>) -> Result<Upload<R>, Rejection> {
        let mut state = self.lock();
        if state.closed {
            return Err(Rejection::Closed);
        }
        let job = state
            .jobs
            .get_mut(id)
            .filter(|job| {
                job.client == client
                    && !job.uploading
                    && matches!(*job.status.borrow(), Status::Reserved)
            })
            .ok_or(Rejection::NoReservation)?;
        job.uploading = true;
        Ok(Upload {
            queue: self.clone(),
            id: id.clone(),
            submitted: false,
        })
    }

    /// Follows the status of job `id`. The job counts as watched while the returned value
    /// lives.
    pub fn watch(&self, id: &JobId) -> Option<Watch<R>> {
        let mut state = self.lock();
        let job = state.jobs.get_mut(id)?;
        job.watchers += 1;
        Some(Watch {
            queue: self.clone(),
            id: id.clone(),
            status: job.status.subscribe(),
        })
    }

    /// The result of job `id`, if it finished with one.
    pub fn result(&self, id: &JobId) -> Option<Arc<R>> {
        match &*self.lock().jobs.get(id)?.status.borrow() {
            Status::Done(result) => Some(Arc::clone(result)),
            _ => None,
        }
    }

    /// Cancels jobs nobody is watching and forgets expired reservations and finished jobs.
    pub fn sweep(&self) {
        let limits = &self.shared.limits;
        let now = Instant::now();
        let mut state = self.lock();
        let mut unwatched = Vec::new();
        state.jobs.retain(|id, job| {
            let idle = now.saturating_duration_since(job.idle_since);
            match &*job.status.borrow() {
                Status::Reserved => job.uploading || idle < limits.upload_start,
                Status::Waiting { .. } | Status::Running { .. } => {
                    if job.watchers == 0 && idle >= limits.unwatched {
                        unwatched.push(id.clone());
                    }
                    true
                }
                _ => idle < limits.keep_finished,
            }
        });
        for id in unwatched {
            tracing::info!(job = %id, "cancelling: nobody is watching");
            state.cancel(&id, CancelReason::Unwatched);
        }
    }

    /// Runs [`sweep`](Self::sweep) every few seconds for as long as the process lives.
    pub fn spawn_sweeper(&self) {
        let queue = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(SWEEP_INTERVAL);
            loop {
                interval.tick().await;
                queue.sweep();
            }
        });
    }

    /// Stops accepting jobs, cancels the ones that have not started, and gives the running
    /// ones up to `grace` to finish before cancelling them too.
    pub async fn shutdown(&self, grace: Duration) {
        {
            let mut state = self.lock();
            state.closed = true;
            let unstarted: Vec<JobId> = state
                .jobs
                .iter()
                .filter(|(_, job)| {
                    matches!(
                        *job.status.borrow(),
                        Status::Reserved | Status::Waiting { .. }
                    )
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in unstarted {
                state.cancel(&id, CancelReason::Shutdown);
            }
        }
        let all = u32::try_from(self.shared.limits.running).unwrap_or(u32::MAX);
        let finished = tokio::time::timeout(grace, self.shared.running.acquire_many(all)).await;
        if finished.is_err() {
            let mut state = self.lock();
            let running: Vec<JobId> = state
                .jobs
                .iter()
                .filter(|(_, job)| matches!(*job.status.borrow(), Status::Running { .. }))
                .map(|(id, _)| id.clone())
                .collect();
            for id in running {
                state.cancel(&id, CancelReason::Shutdown);
            }
        }
    }

    /// Queues the reserved job `id` to run `work` once a running slot frees up.
    fn submit<W, F>(&self, id: &JobId, work: W) -> Result<(), Rejection>
    where
        W: FnOnce(Progress<R>) -> F + Send + 'static,
        F: Future<Output = anyhow::Result<R>> + Send,
    {
        let mut status = {
            let mut state = self.lock();
            if state.closed {
                return Err(Rejection::Closed);
            }
            let job = state.jobs.get_mut(id).ok_or(Rejection::NoReservation)?;
            job.uploading = false;
            job.idle_since = Instant::now();
            let status = job.status.subscribe();
            state.waiting.push_back(id.clone());
            state.number_waiting();
            status
        };
        let queue = self.clone();
        let id = id.clone();
        tokio::spawn(async move {
            let run = async {
                let _slot = queue
                    .shared
                    .running
                    .acquire()
                    .await
                    .expect("the semaphore is never closed");
                queue.lock().start(&id);
                work(Progress {
                    queue: queue.clone(),
                    id: id.clone(),
                })
                .await
            };
            tokio::select! {
                biased;
                // Whoever cancelled the job already recorded why.
                _ = status.wait_for(|status| matches!(status, Status::Cancelled(_))) => {}
                outcome = run => queue.lock().finish(&id, outcome),
            }
        });
        Ok(())
    }

    fn lock(&self) -> MutexGuard<'_, State<R>> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl<R> State<R> {
    fn admits(&self, client: Option<IpAddr>, capacity: usize) -> Result<(), Rejection> {
        if self.closed {
            return Err(Rejection::Closed);
        }
        let mut active = 0;
        for job in self.jobs.values() {
            if job.status.borrow().is_finished() {
                continue;
            }
            if job.client == client {
                return Err(Rejection::ActiveJob);
            }
            active += 1;
        }
        if active >= capacity {
            return Err(Rejection::Full);
        }
        Ok(())
    }

    fn start(&mut self, id: &JobId) {
        self.waiting.retain(|waiting| waiting != id);
        if let Some(job) = self.jobs.get(id) {
            job.status.send_if_modified(|status| {
                let waiting = matches!(status, Status::Waiting { .. });
                if waiting {
                    *status = Status::Running { percent: 0 };
                }
                waiting
            });
        }
        self.number_waiting();
    }

    fn finish(&mut self, id: &JobId, outcome: anyhow::Result<R>) {
        let Some(job) = self
            .jobs
            .get_mut(id)
            .filter(|job| !job.status.borrow().is_finished())
        else {
            return;
        };
        let status = match outcome {
            Ok(result) => Status::Done(Arc::new(result)),
            Err(error) => {
                tracing::warn!(job = %id, "failed: {error:#}");
                Status::Failed
            }
        };
        job.status.send_replace(status);
        job.idle_since = Instant::now();
    }

    fn cancel(&mut self, id: &JobId, reason: CancelReason) {
        self.waiting.retain(|waiting| waiting != id);
        if let Some(job) = self.jobs.get_mut(id)
            && !job.status.borrow().is_finished()
        {
            job.status.send_replace(Status::Cancelled(reason));
            job.idle_since = Instant::now();
        }
        self.number_waiting();
    }

    /// Tells every waiting job its current position.
    fn number_waiting(&self) {
        for (index, id) in self.waiting.iter().enumerate() {
            let position = index + 1;
            if let Some(job) = self.jobs.get(id) {
                job.status.send_if_modified(|status| match status {
                    Status::Waiting { position: current } if *current == position => false,
                    _ => {
                        *status = Status::Waiting { position };
                        true
                    }
                });
            }
        }
    }
}

/// An upload filling a reservation. Dropping it without submitting releases the reservation.
pub struct Upload<R: Send + Sync + 'static> {
    queue: Queue<R>,
    id: JobId,
    submitted: bool,
}

impl<R: Send + Sync + 'static> Upload<R> {
    /// Queues the job to run `work`, which reports progress through the given handle and
    /// returns the result. An error is logged and the job shows as failed.
    pub fn submit<W, F>(mut self, work: W) -> Result<(), Rejection>
    where
        W: FnOnce(Progress<R>) -> F + Send + 'static,
        F: Future<Output = anyhow::Result<R>> + Send,
    {
        self.queue.submit(&self.id, work)?;
        self.submitted = true;
        Ok(())
    }
}

impl<R: Send + Sync + 'static> Drop for Upload<R> {
    fn drop(&mut self) {
        if !self.submitted {
            self.queue.lock().jobs.remove(&self.id);
        }
    }
}

/// Reports a running job's progress.
pub struct Progress<R: Send + Sync + 'static> {
    queue: Queue<R>,
    id: JobId,
}

impl<R: Send + Sync + 'static> Progress<R> {
    /// Sets the progress shown on the job page, 0 to 100.
    pub fn set(&self, percent: u8) {
        if let Some(job) = self.queue.lock().jobs.get(&self.id) {
            job.status.send_if_modified(|status| match status {
                Status::Running { percent: current } if *current != percent => {
                    *current = percent;
                    true
                }
                _ => false,
            });
        }
    }
}

/// Follows a job's status; the job counts as watched while this lives.
pub struct Watch<R: Send + Sync + 'static> {
    queue: Queue<R>,
    id: JobId,
    status: watch::Receiver<Status<R>>,
}

impl<R: Send + Sync + 'static> Watch<R> {
    /// The current status. [`changed`](Self::changed) then waits for a newer one.
    pub fn status(&mut self) -> Status<R> {
        self.status.borrow_and_update().clone()
    }

    /// Waits for the status to change. Returns `false` once the job is forgotten.
    pub async fn changed(&mut self) -> bool {
        self.status.changed().await.is_ok()
    }
}

impl<R: Send + Sync + 'static> Drop for Watch<R> {
    fn drop(&mut self) {
        if let Some(job) = self.queue.lock().jobs.get_mut(&self.id) {
            job.watchers -= 1;
            if job.watchers == 0 {
                job.idle_since = Instant::now();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use tokio::sync::oneshot;

    use super::*;

    type Outcome = anyhow::Result<&'static str>;

    fn queue() -> Queue<&'static str> {
        Queue::new(Limits {
            jobs: 3,
            running: 1,
            upload_start: Duration::from_secs(60),
            unwatched: Duration::from_secs(30),
            keep_finished: Duration::from_secs(600),
        })
    }

    /// A client address in the optional form `client_ip` gives.
    #[allow(clippy::unnecessary_wraps)]
    fn client(n: u8) -> Option<IpAddr> {
        Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, n)))
    }

    /// Reserves and uploads a job for client `n`; its work ends with what the sender sends.
    fn submit(queue: &Queue<&'static str>, n: u8) -> (JobId, oneshot::Sender<Outcome>) {
        let id = queue.reserve(client(n)).unwrap();
        let (finish, finished) = oneshot::channel::<Outcome>();
        queue
            .begin_upload(&id, client(n))
            .unwrap()
            .submit(|_progress| async move { finished.await.unwrap() })
            .unwrap();
        (id, finish)
    }

    fn status(queue: &Queue<&'static str>, id: &JobId) -> String {
        queue.lock().jobs.get(id).map_or_else(
            || "forgotten".to_owned(),
            |job| format!("{:?}", *job.status.borrow()),
        )
    }

    /// Lets spawned jobs run until they wait again.
    async fn settle() {
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn runs_jobs_one_at_a_time_in_upload_order() {
        let queue = queue();
        let (a, finish_a) = submit(&queue, 1);
        let (b, _finish_b) = submit(&queue, 2);
        let (c, _finish_c) = submit(&queue, 3);
        settle().await;
        assert_eq!(status(&queue, &a), "Running { percent: 0 }");
        assert_eq!(status(&queue, &b), "Waiting { position: 1 }");
        assert_eq!(status(&queue, &c), "Waiting { position: 2 }");

        finish_a.send(Ok("a")).unwrap();
        settle().await;
        assert_eq!(status(&queue, &a), "Done(\"a\")");
        assert_eq!(status(&queue, &b), "Running { percent: 0 }");
        assert_eq!(status(&queue, &c), "Waiting { position: 1 }");
        assert_eq!(queue.result(&a).as_deref(), Some(&"a"));
        assert!(queue.result(&b).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn reports_progress_and_failures() {
        let queue = queue();
        let id = queue.reserve(client(1)).unwrap();
        let (fail, failed) = oneshot::channel::<()>();
        queue
            .begin_upload(&id, client(1))
            .unwrap()
            .submit(|progress| async move {
                progress.set(40);
                failed.await.unwrap();
                anyhow::bail!("Wit is down")
            })
            .unwrap();
        settle().await;
        assert_eq!(status(&queue, &id), "Running { percent: 40 }");
        fail.send(()).unwrap();
        settle().await;
        assert_eq!(status(&queue, &id), "Failed");
        assert_eq!(queue.availability(client(1)), Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn limits_jobs_in_total_and_per_client() {
        let queue = queue();
        let first = queue.reserve(client(1)).unwrap();
        assert_eq!(queue.reserve(client(1)), Err(Rejection::ActiveJob));
        queue.reserve(client(2)).unwrap();
        queue.reserve(client(3)).unwrap();
        assert_eq!(queue.availability(client(4)), Err(Rejection::Full));
        assert_eq!(queue.reserve(client(4)), Err(Rejection::Full));

        // An upload that fails releases its reservation.
        drop(queue.begin_upload(&first, client(1)).unwrap());
        assert_eq!(status(&queue, &first), "forgotten");
        queue.reserve(client(4)).unwrap();
        assert_eq!(queue.availability(client(1)), Err(Rejection::Full));
    }

    #[tokio::test(start_paused = true)]
    async fn uploads_fill_only_their_own_reservation_once() {
        let queue = queue();
        let id = queue.reserve(client(1)).unwrap();
        let other: JobId = "0123456789abcdef0123456789abcdef".parse().unwrap();
        assert!(matches!(
            queue.begin_upload(&id, client(2)),
            Err(Rejection::NoReservation)
        ));
        assert!(matches!(
            queue.begin_upload(&other, client(1)),
            Err(Rejection::NoReservation)
        ));
        let upload = queue.begin_upload(&id, client(1)).unwrap();
        assert!(matches!(
            queue.begin_upload(&id, client(1)),
            Err(Rejection::NoReservation)
        ));
        drop(upload);
    }

    #[tokio::test(start_paused = true)]
    async fn forgets_reservations_whose_upload_never_starts() {
        let queue = queue();
        let idle = queue.reserve(client(1)).unwrap();
        let uploading = queue.reserve(client(2)).unwrap();
        let _upload = queue.begin_upload(&uploading, client(2)).unwrap();
        tokio::time::advance(Duration::from_secs(61)).await;
        queue.sweep();
        assert_eq!(status(&queue, &idle), "forgotten");
        assert_eq!(status(&queue, &uploading), "Reserved");
        assert_eq!(queue.availability(client(1)), Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn cancels_jobs_nobody_watches() {
        let queue = queue();
        let (a, finish_a) = submit(&queue, 1);
        let (b, _finish_b) = submit(&queue, 2);
        let watch_b = queue.watch(&b).unwrap();
        settle().await;

        tokio::time::advance(Duration::from_secs(31)).await;
        queue.sweep();
        settle().await;
        assert_eq!(status(&queue, &a), "Cancelled(Unwatched)");
        assert!(finish_a.is_closed(), "the cancelled work is dropped");
        assert_eq!(status(&queue, &b), "Running { percent: 0 }");

        // Closing the last page starts the clock again.
        drop(watch_b);
        tokio::time::advance(Duration::from_secs(29)).await;
        queue.sweep();
        assert_eq!(status(&queue, &b), "Running { percent: 0 }");
        tokio::time::advance(Duration::from_secs(1)).await;
        queue.sweep();
        assert_eq!(status(&queue, &b), "Cancelled(Unwatched)");
    }

    #[tokio::test(start_paused = true)]
    async fn forgets_finished_jobs_after_a_while() {
        let queue = queue();
        let (id, finish) = submit(&queue, 1);
        finish.send(Ok("done")).unwrap();
        settle().await;
        tokio::time::advance(Duration::from_secs(599)).await;
        queue.sweep();
        assert!(queue.result(&id).is_some());
        tokio::time::advance(Duration::from_secs(1)).await;
        queue.sweep();
        assert!(queue.result(&id).is_none());
        assert!(queue.watch(&id).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn watchers_learn_about_changes_until_the_job_is_forgotten() {
        let queue = queue();
        let (id, finish) = submit(&queue, 1);
        let mut watch = queue.watch(&id).unwrap();
        settle().await;
        assert!(matches!(watch.status(), Status::Running { .. }));
        finish.send(Ok("done")).unwrap();
        assert!(watch.changed().await);
        assert!(matches!(watch.status(), Status::Done(_)));
        tokio::time::advance(Duration::from_secs(600)).await;
        queue.sweep();
        assert!(!watch.changed().await);
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_cancels_waiting_jobs_and_lets_running_ones_finish() {
        let queue = queue();
        let (a, finish_a) = submit(&queue, 1);
        let (b, _finish_b) = submit(&queue, 2);
        let reserved = queue.reserve(client(3)).unwrap();
        settle().await;

        let shutdown = tokio::spawn({
            let queue = queue.clone();
            async move { queue.shutdown(Duration::from_secs(100)).await }
        });
        settle().await;
        assert_eq!(status(&queue, &b), "Cancelled(Shutdown)");
        assert_eq!(status(&queue, &reserved), "Cancelled(Shutdown)");
        assert_eq!(queue.reserve(client(4)), Err(Rejection::Closed));
        assert!(!shutdown.is_finished());

        finish_a.send(Ok("a")).unwrap();
        shutdown.await.unwrap();
        assert_eq!(status(&queue, &a), "Done(\"a\")");
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_cancels_running_jobs_after_the_grace_period() {
        let queue = queue();
        let (id, _finish) = submit(&queue, 1);
        settle().await;
        queue.shutdown(Duration::from_secs(10)).await;
        assert_eq!(status(&queue, &id), "Cancelled(Shutdown)");
    }

    #[test]
    fn job_ids_are_32_lowercase_hex_digits() {
        let id = JobId::random();
        assert_eq!(id.to_string().parse::<JobId>().unwrap(), id);
        assert!("0123456789ABCDEF0123456789abcdef".parse::<JobId>().is_err());
        assert!("0123".parse::<JobId>().is_err());
        assert!("../etc/passwd".parse::<JobId>().is_err());
    }
}
