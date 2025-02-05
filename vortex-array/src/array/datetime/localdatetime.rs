use std::sync::Arc;

use arrow_array::{
    ArrayRef as ArrowArrayRef, TimestampMicrosecondArray, TimestampMillisecondArray,
    TimestampNanosecondArray, TimestampSecondArray,
};
use lazy_static::lazy_static;
use vortex_dtype::{DType, ExtDType, ExtID, PType};
use vortex_error::{vortex_bail, vortex_err, VortexError, VortexResult};

use crate::array::datetime::TimeUnit;
use crate::array::extension::ExtensionArray;
use crate::compute::as_arrow::AsArrowArray;
use crate::compute::cast::cast;
use crate::validity::ArrayValidity;
use crate::{Array, ArrayDType, ArrayData, IntoArrayData};

lazy_static! {
    static ref ID: ExtID = ExtID::from(LocalDateTimeArray::ID);
}

pub struct LocalDateTimeArray<'a> {
    ext: ExtensionArray<'a>,
    time_unit: TimeUnit,
}

impl LocalDateTimeArray<'_> {
    pub const ID: &'static str = "vortex.localdatetime";

    pub fn try_new(time_unit: TimeUnit, timestamps: Array) -> VortexResult<Self> {
        if !timestamps.dtype().is_int() {
            vortex_bail!("Timestamps must be an integer array")
        }
        Ok(Self {
            ext: ExtensionArray::new(LocalDateTimeArray::ext_dtype(time_unit), timestamps),
            time_unit,
        })
    }

    pub fn ext_dtype(time_unit: TimeUnit) -> ExtDType {
        ExtDType::new(ID.clone(), Some(time_unit.metadata().clone()))
    }

    pub fn dtype(&self) -> &DType {
        self.ext.dtype()
    }

    pub fn time_unit(&self) -> TimeUnit {
        self.time_unit
    }

    pub fn timestamps(&self) -> Array {
        self.ext.storage()
    }
}

impl<'a> TryFrom<&ExtensionArray<'a>> for LocalDateTimeArray<'a> {
    type Error = VortexError;

    fn try_from(value: &ExtensionArray<'a>) -> Result<Self, Self::Error> {
        LocalDateTimeArray::try_new(
            try_parse_time_unit(value.ext_dtype())?,
            value.storage().clone(),
        )
    }
}

impl AsArrowArray for LocalDateTimeArray<'_> {
    fn as_arrow(&self) -> VortexResult<ArrowArrayRef> {
        // A LocalDateTime maps to an Arrow Timestamp array with no timezone.
        let timestamps = cast(&self.timestamps(), PType::I64.into())?.flatten_primitive()?;
        let validity = timestamps.logical_validity().to_null_buffer()?;
        let buffer = timestamps.scalar_buffer::<i64>();

        Ok(match self.time_unit() {
            TimeUnit::Ns => Arc::new(TimestampNanosecondArray::new(buffer, validity)),
            TimeUnit::Us => Arc::new(TimestampMicrosecondArray::new(buffer, validity)),
            TimeUnit::Ms => Arc::new(TimestampMillisecondArray::new(buffer, validity)),
            TimeUnit::S => Arc::new(TimestampSecondArray::new(buffer, validity)),
        })
    }
}

impl<'a> TryFrom<&Array<'a>> for LocalDateTimeArray<'a> {
    type Error = VortexError;

    fn try_from(value: &Array<'a>) -> Result<Self, Self::Error> {
        let ext = ExtensionArray::try_from(value)?;
        LocalDateTimeArray::try_new(try_parse_time_unit(ext.ext_dtype())?, ext.storage())
    }
}

impl IntoArrayData for LocalDateTimeArray<'_> {
    fn into_array_data(self) -> ArrayData {
        self.ext.into_array_data()
    }
}

fn try_parse_time_unit(ext_dtype: &ExtDType) -> VortexResult<TimeUnit> {
    let byte: [u8; 1] = ext_dtype
        .metadata()
        .ok_or_else(|| vortex_err!("Missing metadata"))?
        .as_ref()
        .try_into()?;
    TimeUnit::try_from(byte[0]).map_err(|_| vortex_err!("Invalid time unit in metadata"))
}
