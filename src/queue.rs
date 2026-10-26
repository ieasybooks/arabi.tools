//! A job queue shared by the tools: how many jobs are accepted and run at once, one active job
//! per client address, live status for job pages, and cleanup of abandoned and finished jobs.
//!
//! A job starts as a reservation ([`Queue::reserve`]) so the browser learns whether there is
//! room before it uploads anything. The upload then fills the reservation ([`Upload::submit`])
//! and the job joins the line of waiting jobs.
//!
//! Queues share running slots ([`Slots`]). When one is free, the job that has waited longest
//! in any queue starts, unless its queue already runs as many jobs as its own limit allows.
//! Within a queue, jobs start from the front of its line, so the order pages show is the
//! order jobs run in.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// How often [`Queue::spawn_sweeper`] looks for jobs to cancel or forget.
const SWEEP_INTERVAL: Duration = Duration::from_secs(5);

/// How many jobs a queue takes and how long it keeps them.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Jobs accepted at once: reserved, waiting and running ones together.
    pub jobs: usize,
    /// Jobs this queue runs at once, within the limit of the [`Slots`] it shares.
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
        if is_random_id(s) {
            Ok(Self(s.to_owned()))
        } else {
            Err(InvalidJobId)
        }
    }
}

/// Whether `s` is written like a random 128-bit id: 32 lowercase hex digits.
fn is_random_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
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

/// Identifies one browser tab's upload attempts; the tab chooses it at random. A tab that
/// lost its reservation, for example by reloading before its upload started, reserves again
/// with the same attempt to replace it. Another tab or browser on the same connection has
/// another attempt, so it cannot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt(String);

impl Attempt {
    /// The attempt written as 32 lowercase hex digits, like a job id.
    pub fn parse(s: &str) -> Option<Self> {
        is_random_id(s).then(|| Self(s.to_owned()))
    }
}

/// Where a job is in its life.
#[derive(Debug)]
pub enum Status<R> {
    /// Reserved; the upload has not arrived yet.
    Reserved,
    /// Waiting for a running slot.
    Waiting {
        /// 1 for the next job to run in its queue.
        position: usize,
    },
    /// Running.
    Running {
        /// Estimated progress, 0 to 100.
        percent: u8,
        /// Completed and total pages, for tools processing documents.
        pages: Option<(usize, usize)>,
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
            Self::Running { percent, pages } => Self::Running {
                percent: *percent,
                pages: *pages,
            },
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
    /// No reservation with this id is waiting for an upload.
    NoReservation,
}

/// Running slots that several queues share, so together they run at most `limit` jobs.
/// Clones share the slots.
#[derive(Clone)]
pub struct Slots {
    /// Held while jobs join a line and while jobs start, so a start always sees every
    /// waiting job. Lock it before a queue, never while holding one.
    scheduler: Arc<Mutex<Scheduler>>,
}

struct Scheduler {
    limit: usize,
    /// The queues sharing the slots.
    queues: Vec<Weak<dyn Line>>,
    /// Numbers jobs in the order they join any queue's line.
    turns: u64,
}

impl Slots {
    pub fn new(limit: usize) -> Self {
        Self {
            scheduler: Arc::new(Mutex::new(Scheduler {
                limit,
                queues: Vec::new(),
                turns: 0,
            })),
        }
    }

    fn join(&self, queue: Weak<dyn Line>) {
        let mut scheduler = self.lock();
        scheduler.queues.retain(|queue| queue.strong_count() > 0);
        scheduler.queues.push(queue);
    }

    fn fill(&self) {
        self.lock().fill();
    }

    fn lock(&self) -> MutexGuard<'_, Scheduler> {
        lock(&self.scheduler)
    }
}

impl Scheduler {
    fn take_turn(&mut self) -> u64 {
        let turn = self.turns;
        self.turns += 1;
        turn
    }

    /// Starts waiting jobs while slots are free, the one that has waited longest first.
    fn fill(&self) {
        let queues: Vec<Arc<dyn Line>> = self.queues.iter().filter_map(Weak::upgrade).collect();
        while queues.iter().map(|queue| queue.running()).sum::<usize>() < self.limit {
            let Some((turn, queue)) = queues
                .iter()
                .filter_map(|queue| Some((queue.next_turn()?, queue)))
                .min_by_key(|(turn, _)| *turn)
            else {
                break;
            };
            // If the job was cancelled since, nothing starts and the next round looks again.
            queue.start(turn);
        }
    }
}

/// What [`Slots`] need from a queue, whatever its jobs produce.
trait Line: Send + Sync {
    /// Jobs the queue runs now.
    fn running(&self) -> usize;
    /// The turn of the job at the front of the line, if the queue may start it now.
    fn next_turn(&self) -> Option<u64>;
    /// Starts the job with `turn`, if it is still at the front of the line and the queue may
    /// start it now.
    fn start(&self, turn: u64);
}

impl<R: Send + Sync> Line for Mutex<State<R>> {
    fn running(&self) -> usize {
        *lock(self).running.borrow()
    }

    fn next_turn(&self) -> Option<u64> {
        lock(self).next_turn()
    }

    fn start(&self, turn: u64) {
        lock(self).start(turn);
    }
}

/// A queue of jobs producing results of type `R`. Clones share the queue.
pub struct Queue<R> {
    state: Arc<Mutex<State<R>>>,
    slots: Slots,
}

impl<R> Clone for Queue<R> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            slots: self.slots.clone(),
        }
    }
}

struct State<R> {
    limits: Limits,
    jobs: HashMap<JobId, Job<R>>,
    /// Waiting jobs with their turns, next to run first. Jobs start in exactly this order.
    waiting: VecDeque<(u64, JobId)>,
    /// How many running slots this queue takes. Shutdown waits for it to reach zero.
    running: watch::Sender<usize>,
    /// Set on shutdown: nothing new is accepted or started.
    closed: bool,
}

struct Job<R> {
    client: Option<IpAddr>,
    /// The attempt that reserved the job, if the browser named one.
    attempt: Option<Attempt>,
    status: watch::Sender<Status<R>>,
    /// Open job pages.
    watchers: usize,
    /// An upload request is filling this reservation.
    uploading: bool,
    /// When the job reached its current stage: reserved, waiting, running or finished. Unused
    /// reservations and finished results expire from here.
    status_since: Instant,
    /// When its last page closed, or when it was submitted. An unfinished job that nobody
    /// watches is cancelled from here.
    unwatched_since: Instant,
    /// Holds a running slot: from when the queue starts the job until its work is dropped.
    has_slot: bool,
}

impl<R: Send + Sync + 'static> Queue<R> {
    /// A queue running its jobs in the shared `slots`.
    pub fn new(limits: Limits, slots: &Slots) -> Self {
        let state = Arc::new(Mutex::new(State {
            limits,
            jobs: HashMap::new(),
            waiting: VecDeque::new(),
            running: watch::Sender::new(0),
            closed: false,
        }));
        let line: Weak<Mutex<State<R>>> = Arc::downgrade(&state);
        slots.join(line);
        Self {
            state,
            slots: slots.clone(),
        }
    }

    /// Whether `client` could reserve a job right now. Its unused reservation does not count:
    /// the attempt that made it may reserve again.
    pub fn availability(&self, client: Option<IpAddr>) -> Result<(), Rejection> {
        self.lock().admits(client, Job::is_unused_reservation)
    }

    /// Reserves a job for `client`, to be filled by an upload. It replaces the client's
    /// unused reservation from the same `attempt`, which the browser has lost.
    pub fn reserve(
        &self,
        client: Option<IpAddr>,
        attempt: Option<Attempt>,
    ) -> Result<JobId, Rejection> {
        let lost = |job: &Job<R>| {
            job.client == client
                && attempt.is_some()
                && job.attempt == attempt
                && job.is_unused_reservation()
        };
        let mut state = self.lock();
        state.admits(client, lost)?;
        state.jobs.retain(|_, job| !lost(job));
        let id = JobId::random();
        state.jobs.insert(
            id.clone(),
            Job {
                client,
                attempt,
                status: watch::Sender::new(Status::Reserved),
                watchers: 0,
                uploading: false,
                status_since: Instant::now(),
                unwatched_since: Instant::now(),
                has_slot: false,
            },
        );
        Ok(id)
    }

    /// Starts filling the reservation `id`. Its random id is what ties the upload to it, so
    /// the upload may arrive from another address, e.g. over IPv6 after reserving over IPv4.
    /// Dropping the returned upload without submitting it releases the reservation.
    pub fn begin_upload(&self, id: &JobId) -> Result<Upload<R>, Rejection> {
        let mut state = self.lock();
        if state.closed {
            return Err(Rejection::Closed);
        }
        let job = state
            .jobs
            .get_mut(id)
            .filter(|job| job.is_unused_reservation())
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

    /// Whether a job still exists and has not ended, without watching or refreshing it.
    pub fn is_unfinished(&self, id: &JobId) -> bool {
        self.lock()
            .jobs
            .get(id)
            .is_some_and(|job| !job.status.borrow().is_finished())
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
        let now = Instant::now();
        let mut state = self.lock();
        let limits = state.limits.clone();
        let mut unwatched_jobs = Vec::new();
        state.jobs.retain(|id, job| {
            let in_status = now.saturating_duration_since(job.status_since);
            match &*job.status.borrow() {
                Status::Reserved => job.uploading || in_status < limits.upload_start,
                Status::Waiting { .. } | Status::Running { .. } => {
                    let unwatched = now.saturating_duration_since(job.unwatched_since);
                    if job.watchers == 0 && unwatched >= limits.unwatched {
                        unwatched_jobs.push(id.clone());
                    }
                    true
                }
                _ => job.has_slot || in_status < limits.keep_finished,
            }
        });
        for id in unwatched_jobs {
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
        let mut running = {
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
            state.running.subscribe()
        };
        let finished = tokio::time::timeout(grace, running.wait_for(|running| *running == 0))
            .await
            .is_ok_and(|finished| finished.is_ok());
        if !finished {
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
        // Cooperative jobs keep their slots until their remote cleanup has completed.
        let _ = running.wait_for(|running| *running == 0).await;
    }

    /// Puts the reserved job `id` in line to run `work`.
    fn submit<W, F>(
        &self,
        id: &JobId,
        cancellation: Option<CancellationToken>,
        work: W,
    ) -> Result<(), Rejection>
    where
        W: FnOnce(Progress<R>) -> F + Send + 'static,
        F: Future<Output = anyhow::Result<R>> + Send,
    {
        let mut status = {
            let mut scheduler = self.slots.lock();
            let status = {
                let mut state = self.lock();
                if state.closed {
                    return Err(Rejection::Closed);
                }
                let job = state.jobs.get_mut(id).ok_or(Rejection::NoReservation)?;
                job.uploading = false;
                job.status_since = Instant::now();
                job.unwatched_since = Instant::now();
                let status = job.status.subscribe();
                state.waiting.push_back((scheduler.take_turn(), id.clone()));
                state.number_waiting();
                status
            };
            scheduler.fill();
            status
        };
        let queue = self.clone();
        let id = id.clone();
        tokio::spawn(async move {
            // Declared before the work, so it is dropped after it.
            let _slot = Slot {
                queue: queue.clone(),
                id: id.clone(),
            };
            // The queue decides when the job starts; it may also be cancelled before then.
            let started = status
                .wait_for(|status| matches!(status, Status::Running { .. }) || status.is_finished())
                .await
                .is_ok_and(|status| matches!(*status, Status::Running { .. }));
            if !started {
                return;
            }
            let work = work(Progress {
                queue: queue.clone(),
                id: id.clone(),
            });
            tokio::pin!(work);
            tokio::select! {
                biased;
                // Whoever cancelled the job already recorded why.
                () = async { let _ = status.wait_for(|status| matches!(status, Status::Cancelled(_))).await; } => {
                    if let Some(cancellation) = cancellation {
                        cancellation.cancel();
                        let _ = work.await;
                    }
                }
                outcome = &mut work => queue.lock().finish(&id, outcome),
            }
        });
        Ok(())
    }

    /// The running slots this queue shares.
    pub fn slots(&self) -> &Slots {
        &self.slots
    }

    fn lock(&self) -> MutexGuard<'_, State<R>> {
        lock(&self.state)
    }
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<R> Job<R> {
    /// Reserved, with no upload filling it.
    fn is_unused_reservation(&self) -> bool {
        !self.uploading && matches!(*self.status.borrow(), Status::Reserved)
    }
}

impl<R> State<R> {
    /// Whether `client` may reserve a job once its jobs that `replaced` picks are replaced.
    fn admits(
        &self,
        client: Option<IpAddr>,
        replaced: impl Fn(&Job<R>) -> bool,
    ) -> Result<(), Rejection> {
        if self.closed {
            return Err(Rejection::Closed);
        }
        let mut active = 0;
        for job in self.jobs.values() {
            if job.status.borrow().is_finished() && !job.has_slot {
                continue;
            }
            if job.client == client {
                if replaced(job) {
                    continue;
                }
                return Err(Rejection::ActiveJob);
            }
            active += 1;
        }
        if active >= self.limits.jobs {
            return Err(Rejection::Full);
        }
        Ok(())
    }

    /// The turn of the job at the front of the line, if this queue may start it now.
    fn next_turn(&self) -> Option<u64> {
        if self.closed || *self.running.borrow() >= self.limits.running {
            return None;
        }
        self.waiting.front().map(|(turn, _)| *turn)
    }

    /// Starts the job with `turn`, if it is still at the front of the line and this queue may
    /// start it now. Only [`Scheduler::fill`] calls this, so the shared limit holds.
    fn start(&mut self, turn: u64) {
        if self.next_turn() != Some(turn) {
            return;
        }
        let Some((_, id)) = self.waiting.pop_front() else {
            return;
        };
        if let Some(job) = self.jobs.get_mut(&id) {
            let waited = job.status_since.elapsed();
            job.status.send_replace(Status::Running {
                percent: 0,
                pages: None,
            });
            job.status_since = Instant::now();
            job.has_slot = true;
            self.running.send_modify(|running| *running += 1);
            tracing::info!(
                job = %id,
                waited_secs = waited.as_secs(),
                running = *self.running.borrow(),
                waiting = self.waiting.len(),
                "started"
            );
        }
        self.number_waiting();
    }

    /// Frees the running slot of job `id`, whose work has been dropped. The caller then lets
    /// the shared slots start the next job.
    fn free_slot(&mut self, id: &JobId) {
        let Some(job) = self.jobs.get_mut(id).filter(|job| job.has_slot) else {
            return;
        };
        job.has_slot = false;
        if matches!(*job.status.borrow(), Status::Running { .. }) {
            // The work stopped without an outcome: it panicked.
            let ran_secs = job.status_since.elapsed().as_secs();
            tracing::error!(job = %id, ran_secs, "stopped without a result");
            job.status.send_replace(Status::Failed);
            job.status_since = Instant::now();
        }
        self.running.send_modify(|running| *running -= 1);
    }

    fn finish(&mut self, id: &JobId, outcome: anyhow::Result<R>) {
        let Some(job) = self
            .jobs
            .get_mut(id)
            .filter(|job| !job.status.borrow().is_finished())
        else {
            return;
        };
        let ran_secs = job.status_since.elapsed().as_secs();
        let status = match outcome {
            Ok(result) => {
                tracing::info!(job = %id, ran_secs, "done");
                Status::Done(Arc::new(result))
            }
            Err(error) => {
                tracing::warn!(job = %id, ran_secs, "failed: {error:#}");
                Status::Failed
            }
        };
        job.status.send_replace(status);
        job.status_since = Instant::now();
    }

    /// Cancels job `id`. A running job keeps its slot until its task has dropped the work.
    fn cancel(&mut self, id: &JobId, reason: CancelReason) {
        self.waiting.retain(|(_, waiting)| waiting != id);
        let Some(job) = self
            .jobs
            .get_mut(id)
            .filter(|job| !job.status.borrow().is_finished())
        else {
            return;
        };
        tracing::info!(
            job = %id,
            in_status_secs = job.status_since.elapsed().as_secs(),
            was_running = matches!(*job.status.borrow(), Status::Running { .. }),
            ?reason,
            "cancelled"
        );
        job.status.send_replace(Status::Cancelled(reason));
        job.status_since = Instant::now();
        self.number_waiting();
    }

    /// Tells every waiting job its current position.
    fn number_waiting(&self) {
        for (index, (_, id)) in self.waiting.iter().enumerate() {
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
        self.queue.submit(&self.id, None, work)?;
        self.submitted = true;
        Ok(())
    }

    /// Cancels cooperatively: work must observe the token and finish its cleanup. Its running
    /// slot and client reservation remain occupied until the future returns.
    pub fn submit_cancellable<W, F>(mut self, work: W) -> Result<(), Rejection>
    where
        W: FnOnce(Progress<R>, CancellationToken) -> F + Send + 'static,
        F: Future<Output = anyhow::Result<R>> + Send,
    {
        let token = CancellationToken::new();
        self.queue
            .submit(&self.id, Some(token.clone()), move |progress| {
                work(progress, token)
            })?;
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

/// Frees a job's running slot when its task ends, after the work has been dropped, however
/// the task ended: finished, cancelled or panicked.
struct Slot<R: Send + Sync + 'static> {
    queue: Queue<R>,
    id: JobId,
}

impl<R: Send + Sync + 'static> Drop for Slot<R> {
    fn drop(&mut self) {
        self.queue.lock().free_slot(&self.id);
        self.queue.slots.fill();
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
                Status::Running {
                    percent: current, ..
                } if *current != percent => {
                    *current = percent;
                    true
                }
                _ => false,
            });
        }
    }

    /// Reports recognized pages. A result becomes 100% only once its downloads are ready.
    pub fn pages(&self, done: usize, total: usize) {
        if let Some(job) = self.queue.lock().jobs.get(&self.id) {
            job.status.send_if_modified(|status| match status {
                Status::Running { percent, pages } if *pages != Some((done, total)) => {
                    *pages = Some((done, total));
                    *percent = u8::try_from(
                        done.saturating_mul(100)
                            .checked_div(total)
                            .unwrap_or(0)
                            .min(99),
                    )
                    .unwrap_or(99);
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
                job.unwatched_since = Instant::now();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use tokio::sync::oneshot;

    use super::*;

    type Outcome<R = &'static str> = anyhow::Result<R>;

    fn queue() -> Queue<&'static str> {
        queue_for(3)
    }

    /// A queue taking `jobs` jobs at once and running one at a time.
    fn queue_for(jobs: usize) -> Queue<&'static str> {
        sharing(&Slots::new(1), jobs, 1)
    }

    /// A queue taking `jobs` jobs at once and running up to `running` of them in `slots`.
    fn sharing<R: Send + Sync + 'static>(slots: &Slots, jobs: usize, running: usize) -> Queue<R> {
        Queue::new(
            Limits {
                jobs,
                running,
                upload_start: Duration::from_secs(60),
                unwatched: Duration::from_secs(30),
                keep_finished: Duration::from_secs(600),
            },
            slots,
        )
    }

    /// A client address in the optional form `client_ip` gives.
    #[allow(clippy::unnecessary_wraps)]
    fn client(n: u8) -> Option<IpAddr> {
        Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, n)))
    }

    /// An attempt, optional like the one a reservation request may name.
    fn attempt(n: u8) -> Option<Attempt> {
        Attempt::parse(&format!("{n:032x}"))
    }

    /// Reserves and uploads a job for client `n`; its work ends with what the sender sends.
    fn submit<R: Send + Sync + 'static>(
        queue: &Queue<R>,
        n: u8,
    ) -> (JobId, oneshot::Sender<Outcome<R>>) {
        let id = queue.reserve(client(n), None).unwrap();
        let (finish, finished) = oneshot::channel::<Outcome<R>>();
        queue
            .begin_upload(&id)
            .unwrap()
            .submit(|_progress| async move { finished.await.unwrap() })
            .unwrap();
        (id, finish)
    }

    fn status<R: fmt::Debug + Send + Sync + 'static>(queue: &Queue<R>, id: &JobId) -> String {
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
        assert_eq!(status(&queue, &a), "Running { percent: 0, pages: None }");
        assert_eq!(status(&queue, &b), "Waiting { position: 1 }");
        assert_eq!(status(&queue, &c), "Waiting { position: 2 }");

        finish_a.send(Ok("a")).unwrap();
        settle().await;
        assert_eq!(status(&queue, &a), "Done(\"a\")");
        assert_eq!(status(&queue, &b), "Running { percent: 0, pages: None }");
        assert_eq!(status(&queue, &c), "Waiting { position: 1 }");
        assert_eq!(queue.result(&a).as_deref(), Some(&"a"));
        assert!(queue.result(&b).is_none());
    }

    // Like request handlers, submit from a task on the runtime's worker thread, which runs
    // the tasks it spawns latest first. Jobs must still start in the order their pages show.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn starts_jobs_in_the_order_their_pages_show() {
        let queue = queue_for(10);
        let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
        let submitting = queue.clone();
        let finishers = tokio::spawn(async move {
            let mut finishers = Vec::new();
            for n in 1..=10 {
                let id = submitting.reserve(client(n), None).unwrap();
                let (finish, finished) = oneshot::channel::<Outcome>();
                let started = started.clone();
                submitting
                    .begin_upload(&id)
                    .unwrap()
                    .submit(move |_progress| async move {
                        started.send(n).unwrap();
                        finished.await.unwrap()
                    })
                    .unwrap();
                let position = match *submitting.lock().jobs[&id].status.borrow() {
                    Status::Running { .. } => 0,
                    Status::Waiting { position } => position,
                    _ => panic!("job {n} neither runs nor waits"),
                };
                assert_eq!(position, usize::from(n) - 1, "job {n}");
                finishers.push(finish);
            }
            finishers
        })
        .await
        .unwrap();
        for (n, finish) in (1..=10).zip(finishers) {
            assert_eq!(starts.recv().await, Some(n));
            finish.send(Ok("done")).unwrap();
        }
    }

    // Tools with different results share the slots. When one frees, the job that has waited
    // longest starts, even if another queue freed the slot.
    #[tokio::test(start_paused = true)]
    async fn a_free_slot_goes_to_the_job_that_has_waited_longest_in_any_queue() {
        let slots = Slots::new(2);
        let transcripts = sharing::<&'static str>(&slots, 10, 2);
        let documents = sharing::<u32>(&slots, 10, 2);
        let (t1, finish_t1) = submit(&transcripts, 1);
        let (_t2, finish_t2) = submit(&transcripts, 2);
        let (d1, _finish_d1) = submit(&documents, 3);
        let (t3, _finish_t3) = submit(&transcripts, 4);
        settle().await;
        assert_eq!(
            status(&transcripts, &t1),
            "Running { percent: 0, pages: None }"
        );
        assert_eq!(status(&documents, &d1), "Waiting { position: 1 }");
        assert_eq!(status(&transcripts, &t3), "Waiting { position: 1 }");

        finish_t1.send(Ok("t1")).unwrap();
        settle().await;
        assert_eq!(
            status(&documents, &d1),
            "Running { percent: 0, pages: None }"
        );
        assert_eq!(status(&transcripts, &t3), "Waiting { position: 1 }");

        finish_t2.send(Ok("t2")).unwrap();
        settle().await;
        assert_eq!(
            status(&transcripts, &t3),
            "Running { percent: 0, pages: None }"
        );
    }

    // A queue that runs as many jobs as its own limit allows leaves the remaining slots to
    // the others, even when its own jobs have waited longer.
    #[tokio::test(start_paused = true)]
    async fn a_queue_runs_no_more_jobs_than_its_own_limit_in_shared_slots() {
        let slots = Slots::new(4);
        let transcripts = sharing::<&'static str>(&slots, 10, 3);
        let documents = sharing::<u32>(&slots, 10, 3);
        let mut finish_transcripts = Vec::new();
        for n in 1..=3 {
            let (id, finish) = submit(&transcripts, n);
            settle().await;
            assert_eq!(
                status(&transcripts, &id),
                "Running { percent: 0, pages: None }"
            );
            finish_transcripts.push(finish);
        }
        let (t4, _finish_t4) = submit(&transcripts, 4);
        let (d1, finish_d1) = submit(&documents, 5);
        let (d2, _finish_d2) = submit(&documents, 6);
        settle().await;
        assert_eq!(status(&transcripts, &t4), "Waiting { position: 1 }");
        assert_eq!(
            status(&documents, &d1),
            "Running { percent: 0, pages: None }"
        );
        assert_eq!(status(&documents, &d2), "Waiting { position: 1 }");

        finish_d1.send(Ok(1)).unwrap();
        settle().await;
        assert_eq!(
            status(&documents, &d2),
            "Running { percent: 0, pages: None }"
        );
        assert_eq!(status(&transcripts, &t4), "Waiting { position: 1 }");

        finish_transcripts.remove(0).send(Ok("t1")).unwrap();
        settle().await;
        assert_eq!(
            status(&transcripts, &t4),
            "Running { percent: 0, pages: None }"
        );
    }

    // The scheduler picks a job by its turn, then starts it. If the job is cancelled in
    // between, the job behind it must not start in its place: an older job in another queue
    // may be next.
    #[tokio::test(start_paused = true)]
    async fn a_job_cancelled_after_being_picked_is_not_replaced_by_the_one_behind_it() {
        let slots = Slots::new(2);
        let transcripts = sharing::<&'static str>(&slots, 10, 2);
        let documents = sharing::<u32>(&slots, 10, 2);
        let (_t1, _finish_t1) = submit(&transcripts, 1);
        let (_d1, finish_d1) = submit(&documents, 2);
        let (t2, _finish_t2) = submit(&transcripts, 3);
        let (d2, _finish_d2) = submit(&documents, 4);
        let (t3, _finish_t3) = submit(&transcripts, 5);
        settle().await;

        // Replay the scheduler's two steps with a cancellation in between.
        let picked = transcripts.lock().next_turn().unwrap();
        transcripts.lock().cancel(&t2, CancelReason::Unwatched);
        transcripts.lock().start(picked);
        assert_eq!(status(&transcripts, &t3), "Waiting { position: 1 }");

        finish_d1.send(Ok(1)).unwrap();
        settle().await;
        assert_eq!(
            status(&documents, &d2),
            "Running { percent: 0, pages: None }"
        );
        assert_eq!(status(&transcripts, &t3), "Waiting { position: 1 }");
    }

    #[tokio::test(start_paused = true)]
    async fn reports_progress_and_failures() {
        let queue = queue();
        let id = queue.reserve(client(1), None).unwrap();
        let (fail, failed) = oneshot::channel::<()>();
        queue
            .begin_upload(&id)
            .unwrap()
            .submit(|progress| async move {
                progress.set(40);
                failed.await.unwrap();
                anyhow::bail!("Wit is down")
            })
            .unwrap();
        settle().await;
        assert_eq!(status(&queue, &id), "Running { percent: 40, pages: None }");
        fail.send(()).unwrap();
        settle().await;
        assert_eq!(status(&queue, &id), "Failed");
        assert_eq!(queue.availability(client(1)), Ok(()));
    }

    #[tokio::test(start_paused = true)]
    async fn limits_jobs_in_total_and_per_client() {
        let queue = queue();
        let first = queue.reserve(client(1), None).unwrap();
        let uploading = queue.begin_upload(&first).unwrap();
        assert_eq!(queue.reserve(client(1), None), Err(Rejection::ActiveJob));
        drop(uploading);
        let first = queue.reserve(client(1), None).unwrap();
        queue.reserve(client(2), None).unwrap();
        queue.reserve(client(3), None).unwrap();
        assert_eq!(queue.availability(client(4)), Err(Rejection::Full));
        assert_eq!(queue.reserve(client(4), None), Err(Rejection::Full));

        // An upload that fails releases its reservation.
        drop(queue.begin_upload(&first).unwrap());
        assert_eq!(status(&queue, &first), "forgotten");
        queue.reserve(client(4), None).unwrap();
        assert_eq!(queue.availability(client(1)), Err(Rejection::Full));
    }

    // A tab can lose its reservation, e.g. when the page reloads before its upload starts;
    // reserving again from the same attempt must not wait for the lost one to expire.
    #[tokio::test(start_paused = true)]
    async fn reserving_again_from_the_same_attempt_replaces_its_unused_reservation() {
        let queue = queue();
        let lost = queue.reserve(client(1), attempt(1)).unwrap();
        assert_eq!(queue.availability(client(1)), Ok(()));
        let retry = queue.reserve(client(1), attempt(1)).unwrap();
        assert_eq!(status(&queue, &lost), "forgotten");
        assert!(matches!(
            queue.begin_upload(&lost),
            Err(Rejection::NoReservation)
        ));

        // A reservation being uploaded, waiting or running still counts.
        let upload = queue.begin_upload(&retry).unwrap();
        assert_eq!(
            queue.reserve(client(1), attempt(1)),
            Err(Rejection::ActiveJob)
        );
        upload.submit(|_progress| async { Ok("done") }).unwrap();
        assert_eq!(
            queue.reserve(client(1), attempt(1)),
            Err(Rejection::ActiveJob)
        );
    }

    // Another tab or browser on the same connection may not take over a reservation whose
    // upload is still on its way.
    #[tokio::test(start_paused = true)]
    async fn other_attempts_from_the_same_client_cannot_replace_a_reservation() {
        let queue = queue();
        let first = queue.reserve(client(1), attempt(1)).unwrap();
        for other in [attempt(2), None] {
            assert_eq!(queue.reserve(client(1), other), Err(Rejection::ActiveJob));
        }
        assert_eq!(status(&queue, &first), "Reserved");
        queue.reserve(client(2), attempt(1)).unwrap();
        assert_eq!(status(&queue, &first), "Reserved");
    }

    #[tokio::test(start_paused = true)]
    async fn uploads_fill_only_existing_reservations_once() {
        let queue = queue();
        let id = queue.reserve(client(1), None).unwrap();
        let other: JobId = "0123456789abcdef0123456789abcdef".parse().unwrap();
        assert!(matches!(
            queue.begin_upload(&other),
            Err(Rejection::NoReservation)
        ));
        let upload = queue.begin_upload(&id).unwrap();
        assert!(matches!(
            queue.begin_upload(&id),
            Err(Rejection::NoReservation)
        ));
        drop(upload);
    }

    #[tokio::test(start_paused = true)]
    async fn forgets_reservations_whose_upload_never_starts() {
        let queue = queue();
        let idle = queue.reserve(client(1), None).unwrap();
        let uploading = queue.reserve(client(2), None).unwrap();
        let _upload = queue.begin_upload(&uploading).unwrap();
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
        assert_eq!(status(&queue, &b), "Running { percent: 0, pages: None }");

        // Closing the last page starts the clock again.
        drop(watch_b);
        tokio::time::advance(Duration::from_secs(29)).await;
        queue.sweep();
        assert_eq!(status(&queue, &b), "Running { percent: 0, pages: None }");
        tokio::time::advance(Duration::from_secs(1)).await;
        queue.sweep();
        assert_eq!(status(&queue, &b), "Cancelled(Unwatched)");
    }

    #[tokio::test(start_paused = true)]
    async fn a_cancelled_job_keeps_its_slot_until_its_work_is_dropped() {
        let queue = queue();
        let (a, finish_a) = submit(&queue, 1);
        let (b, _finish_b) = submit(&queue, 2);
        let _watch_b = queue.watch(&b).unwrap();
        settle().await;

        tokio::time::advance(Duration::from_secs(31)).await;
        queue.sweep();
        assert_eq!(status(&queue, &a), "Cancelled(Unwatched)");
        assert!(!finish_a.is_closed(), "the cancelled work is still alive");
        assert_eq!(status(&queue, &b), "Waiting { position: 1 }");

        settle().await;
        assert!(finish_a.is_closed(), "the cancelled work is dropped");
        assert_eq!(status(&queue, &b), "Running { percent: 0, pages: None }");
    }

    #[tokio::test(start_paused = true)]
    async fn a_job_whose_work_panics_fails_and_frees_its_slot() {
        let queue = queue();
        let id = queue.reserve(client(1), None).unwrap();
        queue
            .begin_upload(&id)
            .unwrap()
            .submit(|_progress| async { panic!("a bug in the tool") })
            .unwrap();
        let (next, _finish_next) = submit(&queue, 2);
        settle().await;
        assert_eq!(status(&queue, &id), "Failed");
        assert_eq!(status(&queue, &next), "Running { percent: 0, pages: None }");
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
    async fn viewing_a_finished_result_does_not_keep_it_longer() {
        let queue = queue();
        let (id, finish) = submit(&queue, 1);
        finish.send(Ok("done")).unwrap();
        settle().await;
        tokio::time::advance(Duration::from_secs(590)).await;
        drop(queue.watch(&id).unwrap());
        tokio::time::advance(Duration::from_secs(10)).await;
        queue.sweep();
        assert!(queue.result(&id).is_none(), "kept for 600 s from finishing");
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
        let reserved = queue.reserve(client(3), None).unwrap();
        settle().await;

        let shutdown = tokio::spawn({
            let queue = queue.clone();
            async move { queue.shutdown(Duration::from_secs(100)).await }
        });
        settle().await;
        assert_eq!(status(&queue, &b), "Cancelled(Shutdown)");
        assert_eq!(status(&queue, &reserved), "Cancelled(Shutdown)");
        assert_eq!(queue.reserve(client(4), None), Err(Rejection::Closed));
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

    #[tokio::test(start_paused = true)]
    async fn cooperative_cancellation_holds_slot_and_client_until_cleanup_finishes() {
        let queue = queue();
        let id = queue.reserve(client(1), None).unwrap();
        let (clean, cleaned) = oneshot::channel();
        let (cancelled, cancellation) = oneshot::channel();
        queue
            .begin_upload(&id)
            .unwrap()
            .submit_cancellable(move |_, token| async move {
                token.cancelled().await;
                cancelled.send(()).unwrap();
                cleaned.await.unwrap();
                Ok("unused result")
            })
            .unwrap();
        let (next, _finish) = submit(&queue, 2);
        let _watch = queue.watch(&next).unwrap();
        settle().await;
        tokio::time::advance(Duration::from_secs(31)).await;
        queue.sweep();
        cancellation.await.unwrap();
        assert_eq!(queue.reserve(client(1), None), Err(Rejection::ActiveJob));
        assert_eq!(status(&queue, &next), "Waiting { position: 1 }");
        // Even expired cancelled records must stay until their occupied slot is released.
        tokio::time::advance(Duration::from_secs(601)).await;
        queue.sweep();
        assert_eq!(status(&queue, &id), "Cancelled(Unwatched)");
        clean.send(()).unwrap();
        settle().await;
        assert_eq!(status(&queue, &next), "Running { percent: 0, pages: None }");
        assert!(queue.result(&id).is_none());
        queue.reserve(client(1), None).unwrap();
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
