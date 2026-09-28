//! Reading and writing the model's parameters and parts.
//!
//! Every write is by id rather than by index, and an id the model does not have
//! is refused. The additive form is what the automatic effects use: a breath
//! contributes to a parameter rather than replacing it, so a model that also
//! drives the same parameter itself does not have the two fighting.

use super::*;

impl CoreModel {
    pub(crate) fn parameter_range(&self, parameter: ProductParameter) -> Option<ParameterRange> {
        self.parameters[parameter.slot()].map(|resolved| resolved.range)
    }
}

impl CoreModel {
    pub(crate) fn parameter_value(
        &self,
        parameter: ProductParameter,
    ) -> Result<Option<f32>, Live2dError> {
        let Some(resolved) = self.parameters[parameter.slot()] else {
            return Ok(None);
        };
        // SAFETY: the parameter table and values belong to the live Model.
        let value = unsafe {
            let count = self.parameter_count()?;
            let values = checked_slice(
                sys::csmGetParameterValues(self.model.as_ptr()),
                count,
                "parameter values",
            )?;
            values[resolved.index]
        };
        if !value.is_finite() {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("{} has a non-finite current value", parameter.id()),
            ));
        }
        Ok(Some(value))
    }
}

impl CoreModel {
    pub(crate) fn set_parameter(
        &mut self,
        parameter: ProductParameter,
        requested: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        if !requested.is_finite() {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                format!("{} received a non-finite value", parameter.id()),
            ));
        }
        let Some(resolved) = self.parameters[parameter.slot()] else {
            return Ok(ParameterUpdate::Unsupported);
        };
        let value = requested.clamp(resolved.range.minimum, resolved.range.maximum);
        // SAFETY: self uniquely owns the live Model and the resolved index was
        // validated against this Model's parameter count during construction.
        unsafe {
            let count = self.parameter_count()?;
            let values = checked_slice_mut(
                sys::csmGetParameterValues(self.model.as_ptr()),
                count,
                "parameter values",
            )?;
            values[resolved.index] = value;
        }
        Ok(ParameterUpdate::Applied {
            value,
            clamped: value != requested,
        })
    }
}

impl CoreModel {
    pub(crate) fn set_parameter_by_id(
        &mut self,
        id: &str,
        requested: f32,
        weight: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        if !requested.is_finite() || !weight.is_finite() || !(0.0..=1.0).contains(&weight) {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                format!("{id} received an invalid motion value or weight"),
            ));
        }
        let Some(resolved) = self.parameters_by_id.get(id).copied() else {
            return Ok(ParameterUpdate::Unsupported);
        };
        // SAFETY: self uniquely owns the Model and the index was validated
        // against this Model's parameter count while building parameters_by_id.
        let value = unsafe {
            let count = self.parameter_count()?;
            let values = checked_slice_mut(
                sys::csmGetParameterValues(self.model.as_ptr()),
                count,
                "parameter values",
            )?;
            let current = values[resolved.index];
            let blended = current + (requested - current) * weight;
            let clamped = blended.clamp(resolved.range.minimum, resolved.range.maximum);
            values[resolved.index] = clamped;
            clamped
        };
        Ok(ParameterUpdate::Applied {
            value,
            clamped: value != requested,
        })
    }
}

impl CoreModel {
    /// Apply an additive parameter contribution, matching Cubism's
    /// `AddParameterValue` operation used by the reference breath layer.
    ///
    /// This is intentionally separate from [`Self::set_parameter_by_id`]:
    /// motion/expression evaluation blends toward a target, while the
    /// reference automatic effects add their weighted contribution to the
    /// current value before the Core range is applied.
    pub(crate) fn add_parameter_by_id(
        &mut self,
        id: &str,
        amount: f32,
        weight: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        if !amount.is_finite() || !weight.is_finite() || !(0.0..=1.0).contains(&weight) {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                format!("{id} received an invalid additive value or weight"),
            ));
        }
        let Some(resolved) = self.parameters_by_id.get(id).copied() else {
            return Ok(ParameterUpdate::Unsupported);
        };
        // SAFETY: self uniquely owns the Model and the index was validated
        // against this Model's parameter count while building parameters_by_id.
        let (value, requested) = unsafe {
            let count = self.parameter_count()?;
            let values = checked_slice_mut(
                sys::csmGetParameterValues(self.model.as_ptr()),
                count,
                "parameter values",
            )?;
            let current = values[resolved.index];
            let requested = current + amount * weight;
            let clamped = requested.clamp(resolved.range.minimum, resolved.range.maximum);
            values[resolved.index] = clamped;
            (clamped, requested)
        };
        Ok(ParameterUpdate::Applied {
            value,
            clamped: value != requested,
        })
    }
}

impl CoreModel {
    pub(crate) fn parameter_range_by_id(&self, id: &str) -> Option<ParameterRange> {
        self.parameters_by_id.get(id).map(|resolved| resolved.range)
    }
}

impl CoreModel {
    pub(crate) fn parameter_value_by_id(&self, id: &str) -> Result<Option<f32>, Live2dError> {
        let Some(resolved) = self.parameters_by_id.get(id).copied() else {
            return Ok(None);
        };
        // SAFETY: self owns the live Model and the resolved index was checked
        // against this Model's parameter array during construction.
        let value = unsafe {
            let count = self.parameter_count()?;
            let values = checked_slice(
                sys::csmGetParameterValues(self.model.as_ptr()),
                count,
                "parameter values",
            )?;
            values[resolved.index]
        };
        if !value.is_finite() {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("{id} has a non-finite current value"),
            ));
        }
        Ok(Some(value))
    }
}

impl CoreModel {
    pub(crate) fn part_opacity_by_id(&self, id: &str) -> Result<Option<f32>, Live2dError> {
        let Some(&index) = self.parts_by_id.get(id) else {
            return Ok(None);
        };
        // SAFETY: the resolved index was validated against this live Model's
        // part arrays while building parts_by_id.
        let value = unsafe {
            let count = self.part_count()?;
            checked_slice(
                sys::csmGetPartOpacities(self.model.as_ptr()),
                count,
                "part opacities",
            )?[index]
        };
        if !value.is_finite() {
            return Err(Live2dError::new(
                Live2dErrorCode::InvalidCoreValue,
                format!("{id} has a non-finite part opacity"),
            ));
        }
        Ok(Some(value))
    }
}

impl CoreModel {
    pub(crate) fn set_part_opacity_by_id(
        &mut self,
        id: &str,
        requested: f32,
        weight: f32,
    ) -> Result<ParameterUpdate, Live2dError> {
        if !requested.is_finite() || !weight.is_finite() || !(0.0..=1.0).contains(&weight) {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                format!("{id} received an invalid part opacity or weight"),
            ));
        }
        let Some(&index) = self.parts_by_id.get(id) else {
            return Ok(ParameterUpdate::Unsupported);
        };
        // SAFETY: self uniquely owns the live Model and the resolved index
        // was validated against its mutable part opacity array.
        let value = unsafe {
            let count = self.part_count()?;
            let opacities = checked_slice_mut(
                sys::csmGetPartOpacities(self.model.as_ptr()),
                count,
                "part opacities",
            )?;
            let current = opacities[index];
            if !current.is_finite() {
                return Err(Live2dError::new(
                    Live2dErrorCode::InvalidCoreValue,
                    format!("{id} has a non-finite part opacity"),
                ));
            }
            let value = (current + (requested - current) * weight).clamp(0.0, 1.0);
            opacities[index] = value;
            value
        };
        Ok(ParameterUpdate::Applied {
            value,
            clamped: value != requested,
        })
    }
}

impl CoreModel {
    pub(crate) fn restore_parameter_defaults(&mut self) -> Result<(), Live2dError> {
        // SAFETY: self uniquely owns the Model. Each parameters_by_id entry
        // was resolved against this exact parameter array during construction.
        unsafe {
            let count = self.parameter_count()?;
            let values = checked_slice_mut(
                sys::csmGetParameterValues(self.model.as_ptr()),
                count,
                "parameter values",
            )?;
            for resolved in self.parameters_by_id.values() {
                values[resolved.index] = resolved.range.default;
            }
        }
        Ok(())
    }
}

impl CoreModel {
    pub(crate) fn restore_part_opacity_defaults(&mut self) -> Result<(), Live2dError> {
        // SAFETY: self uniquely owns the Model. The defaults were copied from
        // this exact part-opacity array while the Model was freshly initialized.
        unsafe {
            let count = self.part_count()?;
            let opacities = checked_slice_mut(
                sys::csmGetPartOpacities(self.model.as_ptr()),
                count,
                "part opacities",
            )?;
            if opacities.len() != self.part_opacity_defaults.len() {
                return Err(Live2dError::new(
                    Live2dErrorCode::InvalidCoreArray,
                    "Core returned a part-opacity array with an unexpected length",
                ));
            }
            opacities.copy_from_slice(&self.part_opacity_defaults);
        }
        Ok(())
    }
}
