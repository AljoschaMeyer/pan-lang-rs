use std::collections::{
    BTreeSet,
    BTreeMap,
};

use gc::{Gc, GcCell};
use gc_derive::{Trace, Finalize};
use ordered_float::OrderedFloat;

use crate::types::{
    rope::Rope,
    bytes::Bytes,
};

/// Runtime representation of an arbitrary pan value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Trace, Finalize)]
pub enum Value {
    Nil,
    Bool(bool),
    Int(i64),
    Float(#[unsafe_ignore_trace] OrderedFloat<f64>),
    Char(char),
    String(Rope),
    Bytes(Bytes),
    Array(Gc<GcCell<Vec<Value>>>),
    Set(Gc<GcCell<BTreeSet<Value>>>),
    Map(Gc<GcCell<BTreeMap<Value, Value>>>),
}
// TODO functions, futures, userdata (light and/or managed?)

impl Value {
    pub fn nil() -> Value {
        Value::Nil
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Nil | Value::Bool(false) => false,
            _ => true,
        }
    }

    // Apply this value to the given args.
    pub fn apply(&self, arg: &[Value]) -> Result<Value, Value> {
        unimplemented!()
    }
}
