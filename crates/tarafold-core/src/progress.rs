//! Progress events for long runs (stage starts and search progress). Library crates emit
//! events; the CLI or server installs a sink that shows them. Without a sink, emitting is a
//! cheap no-op. Events never affect results.

use std::sync::{Mutex, RwLock};

/// One progress update.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    /// What is running, e.g. `discover:revenue`.
    pub stage: String,
    /// Units done (e.g. expression evaluations); 0 when not counted.
    pub done: u64,
    /// Units planned; 0 when unknown.
    pub total: u64,
    /// Short detail, e.g. the best loss so far.
    pub detail: String,
}

type Sink = Box<dyn Fn(&Event) + Send + Sync>;

static SINK: RwLock<Option<Sink>> = RwLock::new(None);
static STAGE: Mutex<String> = Mutex::new(String::new());

/// Install the sink that receives every event (replaces any earlier one).
pub fn set_sink(sink: Option<Sink>) {
    if let Ok(mut s) = SINK.write() {
        *s = sink;
    }
}

/// Whether a sink is installed (lets emitters skip formatting work).
pub fn active() -> bool {
    SINK.read().map(|s| s.is_some()).unwrap_or(false)
}

/// Name the stage that following [`search`] events belong to, and announce it.
pub fn stage(name: &str) {
    if let Ok(mut s) = STAGE.lock() {
        name.clone_into(&mut s);
    }
    send(&Event {
        stage: name.to_string(),
        done: 0,
        total: 0,
        detail: String::new(),
    });
}

/// Search progress within the current stage.
pub fn search(done: u64, total: u64, detail: impl FnOnce() -> String) {
    if !active() {
        return;
    }
    let stage = STAGE.lock().map(|s| s.clone()).unwrap_or_default();
    send(&Event {
        stage,
        done,
        total,
        detail: detail(),
    });
}

fn send(e: &Event) {
    if let Ok(s) = SINK.read()
        && let Some(f) = s.as_ref()
    {
        f(e);
    }
}
