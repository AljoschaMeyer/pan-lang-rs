use futures::future::LocalFutureObj;
use gc::{Gc, GcCell};

use crate::value::Value;

pub struct Job;

pub enum LifecycleState {
    Inert,
    Staged,
    Running,
    Resolved,
    Rejected,
    Cancelled,
}

enum PanFuture {
    Resolve(Resolve),
    Reject(Reject),
    Never(Never),
    OnIdle(OnIdle),
}

// Possible states of a `fut_resolve` future.
enum Resolve {
    Inactive(Value),
    Resolved,
}

// Possible states of a `fut_reject` future.
enum Reject {
    Inactive(Value),
    Rejected,
}

// Possible states of a `fut_never` future.
enum Never {
    Inactive(Job),
    Cancelled,
}

// Possible states of a `fut_on_idle` future.
enum OnIdle {
    Inactive(Job),
    Cancelled,
}

// Represents what can happen when a PanFuture successfully transitions into the pending state.
//
// `ResolveImmediately` and `RejectImmediately` are special cases for the built-in `fut_resolve`
// and `fut_reject` futures to circumvent the event loop. `OnIdle` is a special case for the
// built-in `fut_on_idle` future.
//
// Everything else spawns a rust future on the event loop.
enum Run {
    ResolveImmediately(Value),
    RejectImmediately(Value),
    OnIdle(Job),
    SpawnOnEventLoop(LocalFutureObj<'static, Result<Value, Value>>),
}
// combinators here? Add case for Never to avoid spwaning an actual future on the loop?
