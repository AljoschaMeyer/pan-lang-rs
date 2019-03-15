// The internal representation of pan bytes.

use std::rc::Rc;
use std::cell::RefCell;

use gc_derive::{Trace, Finalize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Trace, Finalize)]
pub struct Bytes {
    #[unsafe_ignore_trace]
    data: Rc<RefCell<[u8]>>,
    start: usize, // inclusive
    end: usize, // exclusive
    // invariant: start and end are always < data.len()
}

impl Bytes {
    pub fn from_slice(b: &[u8]) -> Bytes {
        unimplemented!()
    }
}
