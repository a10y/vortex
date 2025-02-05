use vortex_error::VortexResult;

use crate::array::bool::BoolArray;
use crate::compute::slice::SliceFn;
use crate::{IntoArray, OwnedArray};

impl SliceFn for BoolArray<'_> {
    fn slice(&self, start: usize, stop: usize) -> VortexResult<OwnedArray> {
        BoolArray::try_new(
            self.boolean_buffer().slice(start, stop - start),
            self.validity().slice(start, stop)?,
        )
        .map(|a| a.into_array())
    }
}
