// The internal representation of pan strings. `O(log(n))` all the things!

use gc_derive::{Trace, Finalize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Trace, Finalize)]
pub struct Rope(String); // TODO use actual ropes (but keep String for small strings), make sure cloning is very cheap!
