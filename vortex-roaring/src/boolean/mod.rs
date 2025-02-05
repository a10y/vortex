use arrow_buffer::BooleanBuffer;
use arrow_buffer::Buffer as ArrowBuffer;
use compress::roaring_encode;
use croaring::{Bitmap, Portable};
use serde::{Deserialize, Serialize};
use vortex::array::bool::{Bool, BoolArray};
use vortex::stats::ArrayStatisticsCompute;
use vortex::validity::{ArrayValidity, LogicalValidity, Validity};
use vortex::visitor::{AcceptArrayVisitor, ArrayVisitor};
use vortex::{impl_encoding, ArrayDType, ArrayFlatten, OwnedArray};
use vortex_buffer::Buffer;
use vortex_dtype::Nullability;
use vortex_dtype::Nullability::NonNullable;
use vortex_error::{vortex_bail, vortex_err};
use Nullability::Nullable;

mod compress;
mod compute;

impl_encoding!("vortex.roaring_bool", RoaringBool);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoaringBoolMetadata {
    length: usize,
}

impl RoaringBoolArray<'_> {
    pub fn try_new(bitmap: Bitmap, length: usize) -> VortexResult<Self> {
        if length < bitmap.cardinality() as usize {
            vortex_bail!("RoaringBoolArray length is less than bitmap cardinality")
        } else {
            Ok(Self {
                typed: TypedArray::try_from_parts(
                    DType::Bool(NonNullable),
                    RoaringBoolMetadata { length },
                    Some(Buffer::from(bitmap.serialize::<Portable>())),
                    vec![].into(),
                    StatsSet::new(),
                )?,
            })
        }
    }

    pub fn bitmap(&self) -> Bitmap {
        //TODO(@jdcasale): figure out a way to avoid this deserialization per-call
        Bitmap::deserialize::<Portable>(
            self.array()
                .buffer()
                .expect("RoaringBoolArray buffer is missing")
                .as_ref(),
        )
    }

    pub fn encode(array: OwnedArray) -> VortexResult<OwnedArray> {
        if array.encoding().id() == Bool::ID {
            roaring_encode(BoolArray::try_from(array)?).map(|a| a.into_array())
        } else {
            Err(vortex_err!("RoaringInt can only encode boolean arrays"))
        }
    }
}
impl AcceptArrayVisitor for RoaringBoolArray<'_> {
    fn accept(&self, _visitor: &mut dyn ArrayVisitor) -> VortexResult<()> {
        // TODO(ngates): should we store a buffer in memory? Or delay serialization?
        //  Or serialize into metadata? The only reason we support buffers is so we can write to
        //  the wire without copying into FlatBuffers. But if we need to allocate to serialize
        //  the bitmap anyway, then may as well shove it into metadata.
        todo!()
    }
}

impl ArrayTrait for RoaringBoolArray<'_> {
    fn len(&self) -> usize {
        self.metadata().length
    }
}

impl ArrayStatisticsCompute for RoaringBoolArray<'_> {}

impl ArrayValidity for RoaringBoolArray<'_> {
    fn logical_validity(&self) -> LogicalValidity {
        LogicalValidity::AllValid(self.len())
    }

    fn is_valid(&self, _index: usize) -> bool {
        true
    }
}

impl ArrayFlatten for RoaringBoolArray<'_> {
    fn flatten<'a>(self) -> VortexResult<Flattened<'a>>
    where
        Self: 'a,
    {
        // TODO(ngates): benchmark the fastest conversion from BitMap.
        //  Via bitset requires two copies.
        let bitset = self
            .bitmap()
            .to_bitset()
            .ok_or(vortex_err!("Failed to convert RoaringBitmap to Bitset"))?;

        let bytes = &bitset.as_slice()[0..bitset.size_in_bytes()];
        let buffer = ArrowBuffer::from_slice_ref(bytes);
        Ok(Flattened::Bool(BoolArray::try_new(
            BooleanBuffer::new(buffer, 0, bitset.size_in_bits()),
            match self.dtype().nullability() {
                NonNullable => Validity::NonNullable,
                Nullable => Validity::AllValid,
            },
        )?))
    }
}

#[cfg(test)]
mod test {
    use vortex::array::bool::BoolArray;
    use vortex::compute::scalar_at::scalar_at;
    use vortex::IntoArray;
    use vortex_error::VortexResult;
    use vortex_scalar::Scalar;

    use crate::RoaringBoolArray;

    #[test]
    pub fn iter() -> VortexResult<()> {
        let bool: BoolArray = BoolArray::from(vec![true, false, true, true]);
        let array = RoaringBoolArray::encode(bool.into_array())?;
        let round_trip = RoaringBoolArray::try_from(array.clone())?;
        let values = round_trip.bitmap().to_vec();
        assert_eq!(values, vec![0, 2, 3]);

        Ok(())
    }

    #[test]
    pub fn test_scalar_at() -> VortexResult<()> {
        let bool: BoolArray = BoolArray::from(vec![true, false, true, true]);
        let array = RoaringBoolArray::encode(bool.into_array())?;

        let truthy: Scalar = true.into();
        let falsy: Scalar = false.into();

        assert_eq!(scalar_at(&array, 0)?, truthy);
        assert_eq!(scalar_at(&array, 1)?, falsy);
        assert_eq!(scalar_at(&array, 2)?, truthy);
        assert_eq!(scalar_at(&array, 3)?, truthy);

        Ok(())
    }
}
