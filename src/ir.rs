// The intermediate representation of this pan implementation. Pan functions are compiled into ir
// functions, which are then interpreted.

use std::rc::Rc;
use std::cell::RefCell;

use crate::value::Value;

// What identifiers do in pan, DeBruijnPairs do in the ir.
//
// The `up` field addresses an environment: 0 is the current environment, 1 the parent environment,
// 2 the parent's parent environment, and so on. Within the correct environment, `index` addresses
// the binding.
#[derive(Copy, Clone, PartialEq, Eq)]
struct DeBruijnPair {
    up: usize,
    index: usize,
}

impl DeBruijnPair {
    fn new(up: usize, index: usize) -> DeBruijnPair {
        DeBruijnPair { up, index }
    }
}

// In pan, the environment maps identifiers to values, and mutable bindings can be updated whereas
// immutable bindings can not. In the ir, values are addressed by index, and there are no dynamic
// checks whether mutation is allowed. The compilation step is responsible that identifiers are
// correctly translated to DeBruijnPairs and no disallowed mutations occur. We don't go so far as
// to use unsafe access though, but in theory we could.
struct Environment {
    // The bindings local to this environment.
    bindings: Box<[Value]>,
    // (Mutable) access to the parent binding, which is `None` for the top-level environment.
    parent: Option<Rc<RefCell<Environment>>>,
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
}

// Holds some ir code to be interpreted. Each standalone pan function compiles to an IrFunction.
// Functions of the same `rec` group all share the same IrFunction, this is enables mutual tail
// call optimizations. Different functions of the same `rec` group begin execution of the ir code
// at different offsets. The offset at which to start is part of the runtime values.
struct IrFunction {
    // The maximum number of arguments the function takes. Any additional arguments are ignored.
    // For multiple pan rec functions, this is the maximum over the number of argument of the pan
    // functions.
    args: usize,
    // The maximum number of temporary values this function needs.
    storage_size: usize,
    // The ir code.
    code: Box<[Instruction]>,
}

// Instructions deal with values either in the environment or in the IrFunction's storage. This
// enum can address either.
enum Addr {
    Storage(usize),
    Environment(DeBruijnPair),
}

// A single instruction of ir code. It can operate on the temporary storage and pc, as well as
// on the environment of the execution closure.
enum Instruction {
    // Write the value in `src` to `dst`.
    Write { src: Addr, dst: Addr },
    // Apply the value at `fun` to the first `numArgs` values in the storage and write the return
    // value to `dst`. If the function has thrown: If `catch == NO_CATCH`, rethrow the value, else
    // write the value to storage[0] and set the pc to `catch`.
    Apply { fun: Addr, num_args: usize, dst: Addr, catch: usize },
    // Set the pc to this value.
    Jump(usize),
    // Set the pc to this value if the value at the given Addr is truthy.
    CondJump(Addr, usize),
    // Return the value at the address.
    Return(Addr),
    // Throw the value at the address.
    Throw(Addr),
}

static NO_CATCH: usize = std::usize::MAX;

// An IrFunction together with an environment. This is a runtime value.
pub struct IrClosure {
    env: Rc<RefCell<Environment>>,
    fun: Rc<IrFunction>,
    // The offset at which to begin execution of the `fun`.
    entry: usize,
}

impl IrClosure {
    pub fn run(&self, args: &[Value]) -> Result<Value, Value> {
        let mut storage = Vec::with_capacity(self.fun.storage_size);
        storage.resize(self.fun.storage_size, Value::nil());
        let mut pc = self.entry;

        for (i, arg) in args.iter().enumerate() {
            self.env.borrow_mut().set(DeBruijnPair {
                up: 0,
                index: i,
            }, arg.clone());
        }

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

                Instruction::Apply { fun, num_args, dst, catch } => {
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
                            if *catch == NO_CATCH {
                                return Err(thrown);
                            } else {
                                storage[0] = thrown;
                                pc = *catch;
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

                Instruction::Return(addr) => return Ok(match addr {
                    Addr::Storage(index) => storage[*index].clone(),
                    Addr::Environment(pair) => self.env.borrow().get(*pair),
                }),

                Instruction::Throw(addr) => return Err(match addr {
                    Addr::Storage(index) => storage[*index].clone(),
                    Addr::Environment(pair) => self.env.borrow().get(*pair),
                }),
            }
        }
    }
}
