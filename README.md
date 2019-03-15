# The Pan Programming Language

An imperative, dynamically typed, event-loop based programming language that does not hate you. How hard can it be?

## Values and Types

Pan is dynamically typed. Variables bindings don't have statically known types, instead all values carry their type with them at runtime. The possible types are:

- `nil`: The unit type.
- `bool`: Either `true` or `false`.
- `int`: A signed 64 bit integer.
- `float`: An IEEE 754 double precision (64 bit) floating point number.
- `char`: A unicode scalar value (*not* a code point).
- `string`: A sequence of unicode scalar values.
- `bytes`: A sequence of arbitrary bytes.
- `array`: A sequence of pan values.
- `set`: An unordered collection of pairwise unique pan values.
- `map`: An unordered collection of key-value pairs with pairwise unique keys.
- `function`: A lexically scoped closure.
- `future`: A lazy, cancellable handle to the event loop.

The immutable value types are `nil`, `bool`, `int`, `float`, `char` and `string`. All other values are passed by reference and can be mutated or exhibit other side effects.

### Futures

Note: This is already slightly out of date. TODO:

- merge all end states into single *done* state
- fut_run takes additional arg on_abort: When the future itself encounters an error (e.g. mapping function throws, chain function throws or returns a non-future)
- add leaf future `fut_thunk` that calls the thunk when it transitions to *pending*
- add function `fut_block` to stop execution of the main thread until the future is done
- decide on camelCase vs snake_case...

Pan is a single-threaded language. Instructions are executed sequentially until the last instruction has been reached, the program then terminates. While this model is suitable for batch computations, it does not work for programs that need to interact with the outside world. To support interactive programs, pan provides an event loop. Events from outside (and inside) the computer are queued up in the event loop. A pan program can request to be notified of such events. Futures provide this interface to the event loop.

#### Lifecycle

A future represents a computation that has not finished yet. The lifecycle of a pan future is given in the state machine below, a future is always in exactly one of the six states.

```ascii
         +--------+                       +-----------+---------+------+
    +--->+ staged +------+   +----------->+ resolved  |         |      |
    |    +--------+      |   |            +-----------+         |      |
    |                    v   |            |             settled |      |
+---+---+  fut_run   +---+---+-+          +-----------+         |      |
| inert +----------->+ pending +--------->+ rejected  |         | done |
+-------+            +-------+-+          +-----------+---------+      |
                             |            |                            |
                             |            +-----------+                |
                             +----------->+ cancelled |                |
                             fut_cancel   +-----------+----------------+

```

All futures start in the *inert* state. An inert future is not yet registered with the event loop, it does not try to make progress. Once `fut_run` has been called with the future, it advances to the *pending* state. The future gets enqueued on the event loop. At any point, an event can cause the future to become either *resolved* with a value or *rejected* with a value. Or the pan program might call `fut_cancel` on it moving it to the *cancelled* state. A future is called *settled* when it is either resolved or rejected. A future is called *done* when it is either settled or cancelled. When a future is *done*, it is removed from the event loop. An inert future can also be used in future composition to build up a new parent future. In that case, it transitions to the *staged* state. It becomes pending when its parent future transitions to *pending*.

##### `fut_run(fut [, onResolved] [, onRejected] [, onCancelled])`

Advance the inert future `fut` to the *pending* state. If/when the future is resolved, `onResolved` is called in the next event loop iteration with the resolved value. If/when the future is rejected, `onRejected` is called in the next event loop iteration with the rejected value. If/when the future is cancelled, `onCancelled` is called in the next event loop iteration with the resolved value.

Note that the invoked callback is never invoked synchronously, it always needs to wait for the next tick of the event loop.

###### Errors

Throws a *type error* if any of the following apply:

- `fut` is not a future
- `onResolved` is neither a function nor `nil`
- `onRejected` is neither a function nor `nil`
- `onCancelled` is neither a function nor `nil`

Throws a *future state error* if `fut` is not *inert*.

In any case, no side-effects happen. The future is never spawned to the event loop.

##### `fut_cancel(fut)`

Advance the future *fut* to the *cancelled* state if it was *pending*, do nothing if it was *done*.

###### Errors

Throws a *type error* if `fut` is not a future.  
Throws a *future state error* if `fut` is *inert* or *staged*. The future stays in its state and can still transition to *pending* at a later point.

#### Leaf Futures

Leaf futures are futures that settle due to some outside event. Native modules can provide leaf futures for e.g. timers or inter-process communication. Pan provides three built-in leaf futures which are deterministic (i.e. non-effectful).

##### `fut_never([onCancelled])`

Returns a future that never settles. If it doesn't get cancelled, it never advances beyond the *pending* state. If/when the future is cancelled, `onCancelled` is called.

###### Errors

Throws a *type error* if `fn` is neither a function nor `nil`.  

##### `fut_resolve(x)`

Returns a future that upon being run immediately resolves to `x`.

##### `fut_reject(x)`

Returns a future that upon being run immediately rejects to `x`.

#### Composing Futures

The functions in this section compose futures, in order to perform asynchronous computations in sequence or in parallel. Each of them takes one or more futures as arguments, and returns a new future. The argument futures are called *children*, the returned future is called the *parent*. The following behavior applies to all functions in this section (and is not explicitly mentioned in the function docs):

- when a parent transitions to *pending*, all its children transition to *pending*
- when a parent transitions to *cancelled*, all its children transition to *cancelled*
- upon creating a parent future, all children transition from *inert* to *staged*

##### `fut_map(fut, fn)`

Returns a parent future that resolves to `fn(x)` if/when `fut` resolves to `x`, rejects `x` if/when `fut` rejects `x`, and is cancelled if/when `fut` is cancelled.

###### Errors

Throws a *type error* if `fut` is not a future or `fn` is not a function.  
Throws a *future state error* if `fut` is not *inert*.

##### `fut_map_err(fut, fn)`

Returns a parent future that resolves to `x` if/when `fut` resolves to `x`, rejects `fn(x)` if/when `fut` rejects `x`, and is cancelled if/when `fut` is cancelled.

###### Errors

Throws a *type error* if `fut` is not a future or `fn` is not a function.  
Throws a *future state error* if `fut` is not *inert*.

##### `fut_chain(fut, fn)`

Chain a future after another one if the first one resolves. The second future is created from the resolved value of the first one.

Returns a parent future that rejects `x` if/when `fut` rejects `x`, and is cancelled if/when `fut` is cancelled. If/when `fut` resolves to `x`, `fn` is applied to `x`, and the future returned by `fn` is immediately set to *pending*. The parent future settles like that future. If `fn` returns a non-future value `y`, this acts as if `fn` had returned `fut_resolve(y)`.

###### Errors

Throws a *type error* if `fut` is not a future or `fn` is not a function.  
Throws a *future state error* if `fut` is not *inert*.

##### `fut_chain_err(fut, fn)`

Chain a future after another one if the first one rejects. The second future is created from the rejected value of the first one.

Returns a parent future that resolves to `x` if/when `fut` resolves to `x`, and is cancelled if/when `fut` is cancelled. If/when `fut` rejects `x`, `fn` is applied to `x`, and the future returned by `fn` is immediately set to *pending*. The parent future settles like that future. If `fn` returns a non-future value `y`, this acts as if `fn` had returned `fut_reject(y)`.

###### Errors

Throws a *type error* if `fut` is not a future or `fn` is not a function.  
Throws a *future state error* if `fut` is not *inert*.

##### `fut_join(a, b)`

Wait for two futures to resolve.

Returns a parent future that resolves to `[x, y]` if/when the future `a` resolved to `x` and the future `b` resolved to `y`. If `a` rejects `z`, the parent future also rejects `z` and `b` is cancelled. If `b` rejects `z`, the parent future also rejects `z` and `a` is cancelled. If `a` or `b` is cancelled, the parent future is also cancelled.

###### Errors

Throws a *type error* if `a` or `b` is not a future.
Throws a *future state error* if `a` or `b` is not *inert*.

##### `fut_join_all(arr)`

Wait for many futures to resolve.

Returns a parent future that resolves to an array of the values all the input futures in `arr` have resolved to. If any input future rejects with `z`, the parent future rejects with `z` and all other input futures are cancelled. If/when all child futures are cancelled, the parent future is cancelled as well.

###### Errors

Throws a *type error* if `arr` is not an array, or if it contains an element that is not a future.
Throws a *future state error* if `arr` contains a future that is not *inert*.

##### `fut_select(a, b)`

Let two futures race to resolve first.

Returns a parent future that settles like the first child future to settle. The other child future is cancelled. If/when both child futures are cancelled, the parent future is cancelled as well.

###### Errors

Throws a *type error* if `a` or `b` is not a future.
Throws a *future state error* if `a` or `b` is not *inert*.

##### `fut_select_all(arr)`

Let many futures race to resolve first.

Returns a parent future that settles like the first child future in the array `arr` to settle. The other child futures are cancelled. If/when all child futures are cancelled, the parent future is cancelled as well.

###### Errors

Throws a *type error* if `a` or `b` is not a future.
Throws a *future state error* if `a` or `b` is not *inert*.
