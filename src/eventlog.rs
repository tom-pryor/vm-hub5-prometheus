use std::collections::HashSet;

use tokio::sync::Mutex;
use tracing::Level;

use crate::hub::models::{EventlogEntry, EventlogResponse};
use crate::hub::{HubClient, HubError};

type SeenSet = HashSet<(String, String)>;

/// Tracks which hub eventlog entries have already been observed, so repeated
/// fetches only log entries that are new since the previous fetch. This is
/// the one piece of mutable state shared across requests in an otherwise
/// stateless/per-request codebase.
///
/// A `tokio::sync::Mutex` (not `std::sync::Mutex`) guards the whole
/// fetch+diff+log critical section, including the `.await` on the hub HTTP
/// call, so concurrent triggers (an in-flight background scrape fetch and a
/// direct `/events` call, or two overlapping scrapes) never diff against a
/// stale baseline or race on which result gets recorded last.
#[derive(Debug)]
pub struct EventlogTracker {
    seen: Mutex<Option<SeenSet>>,
}

impl EventlogTracker {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(None),
        }
    }

    /// Fetches the hub's eventlog, logs any newly-appeared entries at
    /// `level`, and records the new state. Waits for its turn if another
    /// fetch is already in flight. Used by `/events`, which must always
    /// answer.
    pub async fn fetch_and_log(
        &self,
        hub: &HubClient,
        level: Level,
    ) -> Result<EventlogResponse, HubError> {
        let mut seen = self.seen.lock().await;
        let response = hub.fetch_eventlog().await?;
        diff_and_log(&mut seen, &response.eventlog, level);
        Ok(response)
    }

    /// Same as `fetch_and_log`, but returns `None` immediately instead of
    /// waiting if a fetch is already in flight — used by the fire-and-forget
    /// `/metrics`-scrape trigger so overlapping scrapes never pile up
    /// concurrent hub requests or race on the dedup state.
    pub async fn fetch_and_log_if_idle(
        &self,
        hub: &HubClient,
        level: Level,
    ) -> Option<Result<EventlogResponse, HubError>> {
        let mut seen = self.seen.try_lock().ok()?;
        let response = match hub.fetch_eventlog().await {
            Ok(r) => r,
            Err(err) => return Some(Err(err)),
        };
        diff_and_log(&mut seen, &response.eventlog, level);
        Some(Ok(response))
    }
}

impl Default for EventlogTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Pure diff+log step, factored out from the locking/fetching above so it's
/// unit-testable without a real hub call or the async mutex. `entries` is in
/// hub order (newest-first); new entries are logged oldest-first. `seen` is
/// replaced wholesale each call (bounded by the hub's response size, ~30-40
/// entries), not appended to, so it never grows unboundedly across fetches.
fn diff_and_log(seen: &mut Option<SeenSet>, entries: &[EventlogEntry], level: Level) {
    let current: SeenSet = entries
        .iter()
        .map(|e| {
            let (time, message) = e.dedup_key();
            (time.to_owned(), message.to_owned())
        })
        .collect();

    let mut new_entries: Vec<&EventlogEntry> = match seen.as_ref() {
        None => entries.iter().collect(),
        Some(previous) => entries
            .iter()
            .filter(|e| {
                let (time, message) = e.dedup_key();
                !previous.contains(&(time.to_owned(), message.to_owned()))
            })
            .collect(),
    };
    new_entries.reverse();

    for entry in new_entries {
        log_at_level(level, entry);
    }
    *seen = Some(current);
}

/// `tracing::event!`'s level must be a compile-time constant (it's spliced
/// into a `static Metadata` initializer per callsite), so a runtime `Level`
/// from config can't be passed to it directly — dispatch to the matching
/// static macro instead.
fn log_at_level(level: Level, entry: &EventlogEntry) {
    match level {
        Level::ERROR => tracing::error!(
            time = %entry.time,
            priority = %entry.priority,
            "{}",
            entry.message
        ),
        Level::WARN => tracing::warn!(
            time = %entry.time,
            priority = %entry.priority,
            "{}",
            entry.message
        ),
        Level::INFO => tracing::info!(
            time = %entry.time,
            priority = %entry.priority,
            "{}",
            entry.message
        ),
        Level::DEBUG => tracing::debug!(
            time = %entry.time,
            priority = %entry.priority,
            "{}",
            entry.message
        ),
        Level::TRACE => tracing::trace!(
            time = %entry.time,
            priority = %entry.priority,
            "{}",
            entry.message
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex as StdMutex};

    use super::*;

    #[derive(Clone, Default)]
    struct BufWriter(Arc<StdMutex<Vec<u8>>>);

    impl io::Write for BufWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufWriter {
        type Writer = Self;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn entry(time: &str, message: &str) -> EventlogEntry {
        EventlogEntry {
            priority: "notice".to_string(),
            time: time.to_string(),
            message: message.to_string(),
        }
    }

    /// `tracing`'s per-callsite interest cache is process-wide, not
    /// per-thread: these tests all log through the same 5 static callsites
    /// (one per `Level` arm in `log_at_level`), so running them concurrently
    /// races on that cache and can silently drop events on one thread while
    /// another succeeds. Serializing just these tests avoids it.
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    /// Runs `f` under a scoped subscriber that writes formatted log lines
    /// into a buffer, and returns the captured text.
    fn capture<F: FnOnce()>(f: F) -> String {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let buf = BufWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(buf.clone())
            .with_max_level(Level::TRACE)
            .without_time()
            .with_target(false)
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, f);
        String::from_utf8(buf.0.lock().unwrap().clone()).unwrap()
    }

    #[test]
    fn first_fetch_logs_entire_backlog_oldest_first() {
        let mut seen = None;
        // hub order: newest-first
        let entries = vec![entry("t2", "second"), entry("t1", "first")];
        let log = capture(|| diff_and_log(&mut seen, &entries, Level::INFO));
        assert_eq!(log.lines().count(), 2);
        assert!(log.find("first").unwrap() < log.find("second").unwrap());
        assert!(seen.is_some());
    }

    #[test]
    fn second_fetch_logs_only_new_entries() {
        let mut seen = None;
        diff_and_log(&mut seen, &[entry("t1", "first")], Level::INFO); // seed baseline
        let entries = vec![entry("t2", "second"), entry("t1", "first")];
        let log = capture(|| diff_and_log(&mut seen, &entries, Level::INFO));
        assert_eq!(log.lines().count(), 1);
        assert!(log.contains("second"));
    }

    #[test]
    fn no_new_entries_logs_nothing() {
        let mut seen = None;
        diff_and_log(&mut seen, &[entry("t1", "first")], Level::INFO);
        let log = capture(|| diff_and_log(&mut seen, &[entry("t1", "first")], Level::INFO));
        assert!(log.is_empty());
    }

    #[test]
    fn priority_change_alone_is_not_a_new_entry() {
        let mut seen = None;
        diff_and_log(&mut seen, &[entry("t1", "first")], Level::INFO);
        let mut changed = entry("t1", "first");
        changed.priority = "critical".to_string();
        let log = capture(|| diff_and_log(&mut seen, &[changed], Level::INFO));
        assert!(log.is_empty());
    }

    #[test]
    fn aged_out_entries_do_not_cause_spurious_relogging() {
        let mut seen = None;
        diff_and_log(&mut seen, &[entry("t1", "old")], Level::INFO);
        let log = capture(|| diff_and_log(&mut seen, &[entry("t2", "new")], Level::INFO));
        assert!(log.contains("new"));
        assert!(!log.contains("old"));
    }
}
