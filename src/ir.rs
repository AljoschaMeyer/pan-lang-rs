// The intermediate representation of this pan implementation. Pan functions are compiled into ir
// functions, which are then interpreted.

use std::collections::{
    BTreeSet,
    BTreeMap,
};
use std::rc::Rc;

use gc::{Gc, GcCell};
use gc_derive::{Trace, Finalize};
use ordered_float::OrderedFloat;

use crate::types::{
    rope::Rope,
    bytes::Bytes,
};
use crate::value::{Value, Fun};

// What identifiers do in pan, DeBruijnPairs do in the ir.
//
// The `up` field addresses an environment: 0 is the current environment, 1 the parent environment,
// 2 the parent's parent environment, and so on. Within the correct environment, `index` addresses
// the binding.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeBruijnPair {
    up: usize,
    index: usize,
}

impl DeBruijnPair {
    pub fn new(up: usize, index: usize) -> DeBruijnPair {
        DeBruijnPair { up, index }
    }
}

// In pan, the environment maps identifiers to values, and mutable bindings can be updated whereas
// immutable bindings can not. In the ir, values are addressed by index, and there are no dynamic
// checks whether mutation is allowed. The compilation step is responsible that identifiers are
// correctly translated to DeBruijnPairs and no disallowed mutations occur. We don't go so far as
// to use unsafe access though, but in theory we could.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Trace, Finalize)]
struct Environment {
    // The bindings local to this environment.
    bindings: Vec<Value>,
    // (Mutable) access to the parent binding, which is `None` for the top-level environment.
    parent: Option<Gc<GcCell<Environment>>>,
}

impl Environment {
    // Look up the value addressed by the given DeBruijnPair. Panics if the address is invalid
    // (which only happens if compilation is buggy).
    fn get(&self, mut addr: DeBruijnPair) -> Value {
        if addr.up == 0 {
            self.bindings[addr.index].clone()
        } else {
            addr.up -= 1;
            self.parent.as_ref().unwrap().borrow().get(addr)
        }
    }

    // Set the value at the given address. Panics if the address is invalid (which only happens if
    // compilation is buggy).
    fn set(&mut self, mut addr: DeBruijnPair, val: Value) {
        if addr.up == 0 {
            self.bindings[addr.index] = val;
        } else {
            addr.up -= 1;
            self.parent.as_ref().unwrap().borrow_mut().set(addr, val);
        }
    }

    fn child(parent: Gc<GcCell<Environment>>, env_size: usize) -> Gc<GcCell<Environment>> {
        let mut bindings = Vec::with_capacity(env_size);
        bindings.resize(env_size, Value::nil());
        Gc::new(GcCell::new(Environment {
            bindings,
            parent: Some(parent),
        }))
    }
}

// Holds some ir code to be interpreted. Each standalone pan function compiles to an IrFunction.
// Functions of the same `rec` group all share the same IrFunction, this is enables mutual tail
// call optimizations. Different functions of the same `rec` group begin execution of the ir code
// at different offsets. The offset at which to start is part of the runtime values.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct IrFunction {
    // The maximum number of arguments the function takes. Any additional arguments are ignored.
    // For multiple pan rec functions, this is the maximum over the number of argument of the pan
    // functions.
    args: usize,
    // The maximum number of temporary values this function needs.
    storage_size: usize,
    // The number of bindings in the environments for this function.
    env_size: usize,
    // The ir code.
    code: Box<[Instruction]>,
}

// Instructions deal with values either in the environment or in the IrFunction's storage. This
// enum can address either.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Addr {
    Storage(usize),
    Environment(DeBruijnPair),
}

// A single instruction of ir code. It can operate on the temporary storage, the pc (offset of the
// next instruction), the `throw` flag and the `catch` offset (where to continue execution when a
// called function throws), as well as on the environment of the executing closure. After executing
// an instruction that does not modify the pc, increment the pc.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Instruction {
    // Write the value in `src` to `dst`.
    Write { src: Addr, dst: Addr },
    // Apply the value at `fun` to the first `numArgs` values in the storage and write the return
    // value to `dst`. If the function has thrown, set the pc to the `catch` address and write the
    // return value to `storage[0]`.
    Apply { fun: Addr, num_args: usize, dst: Addr},
    // Set the pc to this value.
    Jump(usize),
    // Set the pc to this value if the value at the given Addr is truthy.
    CondJump(Addr, usize),
    // Create a value from the literal and write it to the address.
    Literal(IrLiteral, Addr),
    // Set the `throw` flag, indicating that the function should throw instead of returning.
    // This exists to allow tail call optimization when throwing in tail position.
    ThrowFlag,
    // Set the `catch` address.
    Catch(usize),
    // Return the value at the address. If the `throw` flag is set, throw the value instead.
    Return(Addr),
    // Throw the value at the address.
    Throw(Addr),
}

// If the `catch` offset has this value, rethrow rather than continuing execution.
static NO_CATCH: usize = std::usize::MAX;

// The ir pendant to literals in pan source code. Note that pan literals that include expressions
// can not be translated into IrLiterals directly, they are compiled into multiple Instructions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum IrLiteral {
    Nil,
    Bool(bool),
    Int(i64),
    Float(OrderedFloat<f64>),
    Char(char),
    String(Box<str>),
    Bytes(Box<[u8]>),
    Array(Vec<IrLiteral>),
    Set(BTreeSet<IrLiteral>),
    Map(BTreeMap<IrLiteral, IrLiteral>),
    Fun(Rc<IrFunction>, usize),
}

impl IrLiteral {
    fn to_value(&self, env: &Gc<GcCell<Environment>>) -> Value {
        match *self {
            IrLiteral::Nil => Value::Nil,
            IrLiteral::Bool(b) => Value::Bool(b),
            IrLiteral::Int(n) => Value::Int(n),
            IrLiteral::Float(f) => Value::Float(f),
            IrLiteral::Char(c) => Value::Char(c),
            IrLiteral::String(ref s) => Value::String(Rope::from_str(s)),
            IrLiteral::Bytes(ref b) => Value::Bytes(Bytes::from_slice(b)),
            IrLiteral::Array(ref inners) => {
                let arr_val = Gc::new(GcCell::new(Vec::with_capacity(inners.len())));
                {
                    let mut arr_ref = arr_val.borrow_mut();
                    for inner in inners {
                        arr_ref.push(inner.to_value(env));
                    }
                }
                Value::Array(arr_val)
            }
            IrLiteral::Set(ref inners) => {
                let set_val = Gc::new(GcCell::new(BTreeSet::new()));
                {
                    let mut set_ref = set_val.borrow_mut();
                    for inner in inners {
                        set_ref.insert(inner.to_value(env));
                    }
                }
                Value::Set(set_val)
            }
            IrLiteral::Map(ref inners) => {
                let map_val = Gc::new(GcCell::new(BTreeMap::new()));
                {
                    let mut map_ref = map_val.borrow_mut();
                    for (key, val) in inners {
                        map_ref.insert(key.to_value(env), val.to_value(env));
                    }
                }
                Value::Map(map_val)
            }
            IrLiteral::Fun(ref fun, entry) => {
                Value::Fun(Fun::Pan(IrClosure {
                    env: Environment::child(env.clone(), fun.env_size),
                    fun: fun.clone(),
                    entry,
                }))
            }
        }
    }
}

// An IrFunction together with an environment. This is a runtime value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Trace, Finalize)]
pub struct IrClosure {
    env: Gc<GcCell<Environment>>,
    #[unsafe_ignore_trace]
    fun: Rc<IrFunction>,
    // The offset at which to begin execution of the `fun`.
    entry: usize,
}

impl IrClosure {
    pub fn run(&self, args: &[Value]) -> Result<Value, Value> {
        // The local state of this particular execution.
        let mut storage = Vec::with_capacity(self.fun.storage_size);
        storage.resize(self.fun.storage_size, Value::nil());
        let mut pc = self.entry;
        let mut catch = NO_CATCH;
        let mut throw = false;

        // Move the arguments into the environment.
        for (i, arg) in args.iter().take(self.fun.args).enumerate() {
            self.env.borrow_mut().set(DeBruijnPair {
                up: 0,
                index: i,
            }, arg.clone());
        }

        // Execute ir code until a return or throw instruction is hit. This is the part where
        // turing-completeness happens, it is undecidable in general whether this loop terminates.
        loop {
            match &self.fun.code[pc] {
                Instruction::Write { src, dst } => {
                    let val = match src {
                        Addr::Storage(index) => storage[*index].clone(),
                        Addr::Environment(pair) => self.env.borrow().get(*pair),
                    };

                    match dst {
                        Addr::Storage(index) => storage[*index] = val,
                        Addr::Environment(pair) => self.env.borrow_mut().set(*pair, val),
                    }

                    pc += 1;
                }

                Instruction::Apply { fun, num_args, dst} => {
                    let val = match fun {
                        Addr::Storage(index) => storage[*index].clone(),
                        Addr::Environment(pair) => self.env.borrow().get(*pair),
                    };

                    let result = val.apply(&storage[..*num_args]);
                    match result {
                        Ok(returned) => {
                            match dst {
                                Addr::Storage(index) => storage[*index] = returned,
                                Addr::Environment(pair) => self.env.borrow_mut().set(*pair, returned),
                            }

                            pc += 1;
                        }

                        Err(thrown) => {
                            if catch == NO_CATCH {
                                return Err(thrown);
                            } else {
                                storage[0] = thrown;
                                pc = catch;
                            }
                        }
                    }
                }

                Instruction::Jump(new_pc) => pc = *new_pc,

                Instruction::CondJump(addr, new_pc) => {
                    let val = match addr {
                        Addr::Storage(index) => storage[*index].clone(),
                        Addr::Environment(pair) => self.env.borrow().get(*pair),
                    };

                    if val.truthy() {
                        pc = *new_pc;
                    } else {
                        pc += 1;
                    }
                }

                Instruction::Literal(lit, dst) => {
                    match dst {
                        Addr::Storage(index) => storage[*index] = lit.to_value(&self.env),
                        Addr::Environment(pair) => self.env.borrow_mut().set(*pair, lit.to_value(&self.env)),
                    }

                    pc += 1;
                }

                Instruction::ThrowFlag => {
                    throw = true;
                    pc += 1;
                }

                Instruction::Catch(offset) => {
                    catch = *offset;
                    pc += 1;
                }

                Instruction::Return(addr) => {
                    if throw {
                        return Err(match addr {
                            Addr::Storage(index) => storage[*index].clone(),
                            Addr::Environment(pair) => self.env.borrow().get(*pair),
                        });
                    } else {
                        return Ok(match addr {
                            Addr::Storage(index) => storage[*index].clone(),
                            Addr::Environment(pair) => self.env.borrow().get(*pair),
                        });
                    }
                }

                Instruction::Throw(addr) => return Err(match addr {
                    Addr::Storage(index) => storage[*index].clone(),
                    Addr::Environment(pair) => self.env.borrow().get(*pair),
                }),
            }
        }
    }
}
