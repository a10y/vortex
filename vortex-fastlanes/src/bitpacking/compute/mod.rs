use std::cmp::min;

use fastlanez::TryBitPack;
use itertools::Itertools;
use vortex::array::constant::ConstantArray;
use vortex::array::primitive::PrimitiveArray;
use vortex::array::sparse::SparseArray;
use vortex::compute::scalar_at::{scalar_at, ScalarAtFn};
use vortex::compute::slice::{slice, SliceFn};
use vortex::compute::take::{take, TakeFn};
use vortex::compute::ArrayCompute;
use vortex::{Array, ArrayDType, ArrayTrait, IntoArray, OwnedArray};
use vortex_dtype::{match_each_integer_ptype, NativePType};
use vortex_error::{vortex_bail, vortex_err, VortexResult};
use vortex_scalar::Scalar;

use crate::bitpacking::compress::unpack_single;
use crate::{match_integers_by_width, unpack_single_primitive, BitPackedArray};

mod slice;

impl ArrayCompute for BitPackedArray<'_> {
    fn scalar_at(&self) -> Option<&dyn ScalarAtFn> {
        Some(self)
    }

    fn slice(&self) -> Option<&dyn SliceFn> {
        Some(self)
    }

    fn take(&self) -> Option<&dyn TakeFn> {
        Some(self)
    }
}

impl ScalarAtFn for BitPackedArray<'_> {
    fn scalar_at(&self, index: usize) -> VortexResult<Scalar> {
        if index >= self.len() {
            return Err(vortex_err!(OutOfBounds: index, 0, self.len()));
        }
        if let Some(patches) = self.patches() {
            // NB: All non-null values are considered patches
            if self.bit_width() == 0 || patches.with_dyn(|a| a.is_valid(index)) {
                return scalar_at(&patches, index)?.cast(self.dtype());
            }
        }
        unpack_single(self, index)?.cast(self.dtype())
    }
}

impl TakeFn for BitPackedArray<'_> {
    fn take(&self, indices: &Array) -> VortexResult<OwnedArray> {
        let ptype = self.dtype().try_into()?;
        let validity = self.validity();
        let taken_validity = validity.take(indices)?;
        if self.bit_width() == 0 {
            return if let Some(patches) = self.patches() {
                let primitive_patches = take(&patches, indices)?.flatten_primitive()?;
                Ok(primitive_patches.into_array())
            } else {
                Ok(
                    ConstantArray::new(Scalar::null(&self.dtype().as_nullable()), indices.len())
                        .into_array(),
                )
            };
        }

        let indices = indices.clone().flatten_primitive()?;
        let taken = match_integers_by_width!(ptype, |$T| {
            PrimitiveArray::from_vec(take_primitive::<$T>(self, &indices)?, taken_validity)
        });
        Ok(taken.reinterpret_cast(ptype).into_array())
    }
}

fn take_primitive<T: NativePType + TryBitPack>(
    array: &BitPackedArray,
    indices: &PrimitiveArray,
) -> VortexResult<Vec<T>> {
    // Group indices into 1024-element chunks and relativise them to the beginning of each chunk
    let relative_indices: Vec<(usize, Vec<u16>)> = match_each_integer_ptype!(indices.ptype(), |$P| {
        indices
            .typed_data::<$P>()
            .iter()
            .group_by(|idx| (**idx / 1024) as usize)
            .into_iter()
            .map(|(k, g)| (k, g.map(|idx| (*idx % 1024) as u16).collect()))
            .collect()
    });

    let bit_width = array.bit_width();
    let packed = array.packed().flatten_primitive()?;
    let packed = packed.typed_data::<u8>();

    let patches = array.patches().map(SparseArray::try_from).transpose()?;

    // if we have a small number of relatively large batches, we gain by slicing and then patching inside the loop
    // if we have a large number of relatively small batches, the overhead isn't worth it, and we're better off with a bulk patch
    // roughly, if we have an average of less than 64 elements per batch, we prefer bulk patching
    let prefer_bulk_patch = relative_indices.len() * 64 > indices.len();

    // assuming the buffer is already allocated (which will happen at most once)
    // then unpacking all 1024 elements takes ~8.8x as long as unpacking a single element
    // see https://github.com/fulcrum-so/vortex/pull/190#issue-2223752833
    // however, the gap should be smaller with larger registers (e.g., AVX-512) vs the 128 bit
    // ones on M2 Macbook Air.
    let unpack_chunk_threshold = 8;

    let mut output = Vec::with_capacity(indices.len());
    let mut buffer: Vec<T> = Vec::new();
    for (chunk, offsets) in relative_indices {
        let packed_chunk = &packed[chunk * 128 * bit_width..][..128 * bit_width];
        if offsets.len() > unpack_chunk_threshold {
            buffer.clear();
            TryBitPack::try_unpack_into(packed_chunk, bit_width, &mut buffer)
                .map_err(|_| vortex_err!("Unsupported bit width {}", bit_width))?;
            for index in &offsets {
                output.push(buffer[*index as usize]);
            }
        } else {
            for index in &offsets {
                output.push(unsafe {
                    unpack_single_primitive::<T>(packed_chunk, bit_width, *index as usize)?
                });
            }
        }

        if !prefer_bulk_patch {
            if let Some(ref patches) = patches {
                let patches_slice = slice(
                    patches.array(),
                    chunk * 1024,
                    min((chunk + 1) * 1024, patches.len()),
                )?;
                let patches_slice = SparseArray::try_from(patches_slice)?;
                let offsets = PrimitiveArray::from(offsets);
                do_patch_for_take_primitive(&patches_slice, &offsets, &mut output)?;
            }
        }
    }

    if prefer_bulk_patch {
        if let Some(ref patches) = patches {
            do_patch_for_take_primitive(patches, indices, &mut output)?;
        }
    }

    Ok(output)
}

fn do_patch_for_take_primitive<T: NativePType + TryBitPack>(
    patches: &SparseArray,
    indices: &PrimitiveArray,
    output: &mut [T],
) -> VortexResult<()> {
    let taken_patches = take(patches.array(), indices.array())?;
    let taken_patches = SparseArray::try_from(taken_patches)?;

    let base_index = output.len() - indices.len();
    let output_patches = taken_patches
        .values()
        .flatten_primitive()?
        .reinterpret_cast(T::PTYPE);
    taken_patches
        .resolved_indices()
        .iter()
        .map(|idx| base_index + *idx)
        .zip_eq(output_patches.typed_data::<T>())
        .for_each(|(idx, val)| {
            output[idx] = *val;
        });

    Ok(())
}

#[cfg(test)]
mod test {
    use itertools::Itertools;
    use rand::distributions::Uniform;
    use rand::{thread_rng, Rng};
    use vortex::array::primitive::{Primitive, PrimitiveArray};
    use vortex::array::sparse::SparseArray;
    use vortex::compress::Compressor;
    use vortex::compute::scalar_at::scalar_at;
    use vortex::compute::take::take;
    use vortex::{ArrayDef, Context, IntoArray};

    use crate::{BitPackedArray, BitPackedEncoding};

    fn ctx() -> Context {
        Context::default().with_encoding(&BitPackedEncoding)
    }

    #[test]
    fn take_indices() {
        let indices = PrimitiveArray::from(vec![0, 125, 2047, 2049, 2151, 2790]);
        let unpacked = PrimitiveArray::from((0..4096).map(|i| (i % 63) as u8).collect::<Vec<_>>());
        let bitpacked = Compressor::new(&ctx())
            .compress(unpacked.array(), None)
            .unwrap();
        let result = take(&bitpacked, indices.array()).unwrap();
        assert_eq!(result.encoding().id(), Primitive::ID);
        let primitive_result = result.flatten_primitive().unwrap();
        let res_bytes = primitive_result.typed_data::<u8>();
        assert_eq!(res_bytes, &[0, 62, 31, 33, 9, 18]);
    }

    #[test]
    fn take_random_indices() {
        let num_patches: usize = 128;
        let values = (0..u16::MAX as u32 + num_patches as u32).collect::<Vec<_>>();
        let uncompressed = PrimitiveArray::from(values.clone());
        let packed = BitPackedArray::encode(uncompressed.array(), 16).unwrap();
        assert!(packed.patches().is_some());

        let patches = SparseArray::try_from(packed.patches().unwrap()).unwrap();
        assert_eq!(
            patches.resolved_indices(),
            ((values.len() + 1 - num_patches)..values.len()).collect_vec()
        );

        let rng = thread_rng();
        let range = Uniform::new(0, values.len());
        let random_indices: PrimitiveArray = rng
            .sample_iter(range)
            .take(10_000)
            .map(|i| i as u32)
            .collect_vec()
            .into();
        let taken = take(packed.array(), random_indices.array()).unwrap();

        // sanity check
        random_indices
            .typed_data::<u32>()
            .iter()
            .enumerate()
            .for_each(|(ti, i)| {
                assert_eq!(
                    u32::try_from(scalar_at(packed.array(), *i as usize).unwrap()).unwrap(),
                    values[*i as usize]
                );
                assert_eq!(
                    u32::try_from(scalar_at(&taken, ti).unwrap()).unwrap(),
                    values[*i as usize]
                );
            });
    }

    #[test]
    fn test_scalar_at() {
        let values = (0u32..257).collect_vec();
        let uncompressed = PrimitiveArray::from(values.clone()).into_array();
        let packed = BitPackedArray::encode(&uncompressed, 8).unwrap();
        assert!(packed.patches().is_some());

        let patches = SparseArray::try_from(packed.patches().unwrap()).unwrap();
        assert_eq!(patches.resolved_indices(), vec![256]);

        values.iter().enumerate().for_each(|(i, v)| {
            assert_eq!(
                u32::try_from(scalar_at(packed.array(), i).unwrap()).unwrap(),
                *v
            );
        });
    }
}
