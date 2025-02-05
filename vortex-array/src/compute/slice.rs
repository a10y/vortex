use vortex_error::{vortex_bail, vortex_err, VortexResult};

use crate::{Array, OwnedArray};

/// Limit array to start..stop range
pub trait SliceFn {
    fn slice(&self, start: usize, stop: usize) -> VortexResult<OwnedArray>;
}

pub fn slice(array: &Array, start: usize, stop: usize) -> VortexResult<OwnedArray> {
    check_slice_bounds(array, start, stop)?;

    array.with_dyn(|c| {
        c.slice().map(|t| t.slice(start, stop)).unwrap_or_else(|| {
            Err(vortex_err!(
                NotImplemented: "slice",
                array.encoding().id()
            ))
        })
    })
}

fn check_slice_bounds(array: &Array, start: usize, stop: usize) -> VortexResult<()> {
    if start > array.len() {
        vortex_bail!(OutOfBounds: start, 0, array.len());
    }
    if stop > array.len() {
        vortex_bail!(OutOfBounds: stop, 0, array.len());
    }
    Ok(())
}
