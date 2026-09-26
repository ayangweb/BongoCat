//! Whether each declared resource is one this renderer can actually show.
//!
//! The schema says a model has a display; it does not say the display has
//! drawables with texture indices that exist. A reference is only meaningful
//! against the inventory that was actually read, so these checks run against
//! both together and name the reference that failed.

use super::*;

pub(crate) fn validate_display_info_resource(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
) -> Result<(), ModelError> {
    let display: RawDisplayInfo = read_json(
        path,
        reference,
        maximum_bytes,
        maximum_depth,
        ModelDiagnostic::ModelResourceInvalid,
    )?;
    if display.version != 3 {
        return invalid_resource(reference, "cdi3 Version must be 3");
    }

    let parameter_groups = display
        .parameter_groups
        .iter()
        .map(|group| group.id.as_str())
        .collect::<BTreeSet<_>>();
    if parameter_groups.len() != display.parameter_groups.len()
        || display
            .parameter_groups
            .iter()
            .any(|group| group.id.trim().is_empty())
    {
        return invalid_resource(
            reference,
            "cdi3 ParameterGroups contain blank or duplicate Id",
        );
    }
    let parameters = display
        .parameters
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect::<BTreeSet<_>>();
    if parameters.len() != display.parameters.len()
        || display
            .parameters
            .iter()
            .any(|parameter| parameter.id.trim().is_empty())
    {
        return invalid_resource(reference, "cdi3 Parameters contain blank or duplicate Id");
    }
    if display.parameters.iter().any(|parameter| {
        !parameter.group_id.is_empty() && !parameter_groups.contains(parameter.group_id.as_str())
    }) {
        return invalid_resource(reference, "cdi3 Parameter GroupId is not declared");
    }
    let parts = display
        .parts
        .iter()
        .map(|part| part.id.as_str())
        .collect::<BTreeSet<_>>();
    if parts.len() != display.parts.len()
        || display.parts.iter().any(|part| part.id.trim().is_empty())
    {
        return invalid_resource(reference, "cdi3 Parts contain blank or duplicate Id");
    }
    if display.combined_parameters.iter().any(|combination| {
        combination.is_empty()
            || combination
                .iter()
                .any(|id| id.trim().is_empty() || !parameters.contains(id.as_str()))
    }) {
        return invalid_resource(
            reference,
            "cdi3 CombinedParameters contain blank or undeclared parameter Ids",
        );
    }
    Ok(())
}

pub(crate) fn validate_expression_resource(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
) -> Result<(), ModelError> {
    let expression: RawExpressionResource = read_json(
        path,
        reference,
        maximum_bytes,
        maximum_depth,
        ModelDiagnostic::ModelResourceInvalid,
    )?;
    if expression.kind != "Live2D Expression" {
        return invalid_resource(reference, "exp3 Type must be Live2D Expression");
    }
    if [expression.fade_in_seconds, expression.fade_out_seconds]
        .into_iter()
        .flatten()
        .any(|seconds| !seconds.is_finite() || seconds < 0.0)
    {
        return invalid_resource(
            reference,
            "exp3 fade duration must be finite and non-negative",
        );
    }
    let parameter_ids = expression
        .parameters
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect::<BTreeSet<_>>();
    if parameter_ids.len() != expression.parameters.len()
        || expression
            .parameters
            .iter()
            .any(|parameter| parameter.id.trim().is_empty() || !parameter.value.is_finite())
    {
        return invalid_resource(reference, "exp3 Parameters contain an invalid Id or Value");
    }
    for parameter in expression.parameters {
        let _ = parameter.blend;
    }
    Ok(())
}

pub(crate) fn validate_motion_resource(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
) -> Result<(), ModelError> {
    let motion: RawMotionResourceFile = read_json(
        path,
        reference,
        maximum_bytes,
        maximum_depth,
        ModelDiagnostic::ModelResourceInvalid,
    )?;
    if motion.version != 3 {
        return invalid_resource(reference, "motion3 Version must be 3");
    }
    if !motion.meta.duration.is_finite()
        || motion.meta.duration < 0.0
        || !motion.meta.fps.is_finite()
        || motion.meta.fps <= 0.0
    {
        return invalid_resource(
            reference,
            "motion3 Meta Duration/Fps must be finite and positive",
        );
    }
    if motion.meta.curve_count != motion.curves.len()
        || motion.meta.user_data_count != motion.user_data.len()
    {
        return invalid_resource(
            reference,
            "motion3 Meta counts do not match declared arrays",
        );
    }
    let user_data_size = motion.user_data.iter().try_fold(0_usize, |total, entry| {
        if !entry.time.is_finite()
            || entry.time < 0.0
            || entry.time > motion.meta.duration + MOTION_TIME_TOLERANCE
        {
            return Err(());
        }
        total.checked_add(entry.value.len()).ok_or(())
    });
    if user_data_size.ok() != Some(motion.meta.total_user_data_size) {
        return invalid_resource(
            reference,
            "motion3 UserData metadata contains an invalid time or size",
        );
    }
    for curve in motion.curves {
        let _ = curve.target;
        if curve.id.trim().is_empty()
            || curve.segments.len() < 2
            || curve.segments.iter().any(|value| !value.is_finite())
            || [curve.fade_in_seconds, curve.fade_out_seconds]
                .into_iter()
                .flatten()
                .any(|seconds| !seconds.is_finite() || seconds < 0.0)
        {
            return invalid_resource(reference, "motion3 curve contains invalid values");
        }
    }
    Ok(())
}

pub(crate) fn validate_pose_resource(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
) -> Result<(), ModelError> {
    let pose: RawPoseResource = read_json(
        path,
        reference,
        maximum_bytes,
        maximum_depth,
        ModelDiagnostic::ModelResourceInvalid,
    )?;
    if pose.kind != "Live2D Pose" {
        return invalid_resource(reference, "pose3 Type must be Live2D Pose");
    }
    if pose
        .fade_in_seconds
        .is_some_and(|seconds| !seconds.is_finite() || seconds < 0.0)
    {
        return invalid_resource(
            reference,
            "pose3 FadeInTime must be finite and non-negative",
        );
    }
    if pose.groups.is_empty() {
        return invalid_resource(reference, "pose3 Groups must contain at least one group");
    }

    let mut part_ids = BTreeSet::new();
    for group in pose.groups {
        if group.is_empty() {
            return invalid_resource(reference, "pose3 groups must not be empty");
        }
        for part in group {
            if part.id.trim().is_empty() || !part_ids.insert(part.id.clone()) {
                return invalid_resource(reference, "pose3 part Id must be non-empty and unique");
            }
            let mut links = BTreeSet::new();
            for link in part.links {
                if link.trim().is_empty() || link == part.id || !links.insert(link) {
                    return invalid_resource(
                        reference,
                        "pose3 Link must be non-empty, unique, and not self-referential",
                    );
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_physics_resource(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
) -> Result<(), ModelError> {
    let physics: RawPhysicsResource = read_json(
        path,
        reference,
        maximum_bytes,
        maximum_depth,
        ModelDiagnostic::ModelResourceInvalid,
    )?;
    validate_physics_resource_value(&physics, reference)
}

pub(crate) fn load_physics_definition(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
) -> Result<PhysicsDefinition, ModelError> {
    let physics: RawPhysicsResource = read_json(
        path,
        reference,
        maximum_bytes,
        maximum_depth,
        ModelDiagnostic::ModelResourceInvalid,
    )?;
    validate_physics_resource_value(&physics, reference)?;
    Ok(PhysicsDefinition {
        fps: physics.meta.fps.unwrap_or(0.0),
        gravity: PhysicsVector {
            x: physics.meta.effective_forces.gravity.x,
            y: physics.meta.effective_forces.gravity.y,
        },
        wind: PhysicsVector {
            x: physics.meta.effective_forces.wind.x,
            y: physics.meta.effective_forces.wind.y,
        },
        settings: physics
            .settings
            .into_iter()
            .map(|setting| PhysicsSetting {
                inputs: setting
                    .inputs
                    .into_iter()
                    .map(|input| PhysicsInput {
                        parameter_id: input.source.id,
                        weight: input.weight,
                        channel: match input.kind {
                            RawPhysicsChannel::X => PhysicsChannel::X,
                            RawPhysicsChannel::Y => PhysicsChannel::Y,
                            RawPhysicsChannel::Angle => PhysicsChannel::Angle,
                        },
                        reflect: input.reflect,
                    })
                    .collect(),
                outputs: setting
                    .outputs
                    .into_iter()
                    .map(|output| PhysicsOutput {
                        parameter_id: output.destination.id,
                        vertex_index: output.vertex_index,
                        scale: output.scale,
                        weight: output.weight,
                        channel: match output.kind {
                            RawPhysicsChannel::X => PhysicsChannel::X,
                            RawPhysicsChannel::Y => PhysicsChannel::Y,
                            RawPhysicsChannel::Angle => PhysicsChannel::Angle,
                        },
                        reflect: output.reflect,
                    })
                    .collect(),
                vertices: setting
                    .vertices
                    .into_iter()
                    .map(|vertex| PhysicsVertex {
                        position: PhysicsVector {
                            x: vertex.position.x,
                            y: vertex.position.y,
                        },
                        mobility: vertex.mobility,
                        delay: vertex.delay,
                        acceleration: vertex.acceleration,
                        radius: vertex.radius,
                    })
                    .collect(),
                normalization_position: PhysicsRange {
                    minimum: setting.normalization.position.minimum,
                    default: setting.normalization.position.default,
                    maximum: setting.normalization.position.maximum,
                },
                normalization_angle: PhysicsRange {
                    minimum: setting.normalization.angle.minimum,
                    default: setting.normalization.angle.default,
                    maximum: setting.normalization.angle.maximum,
                },
            })
            .collect(),
    })
}

pub(crate) fn validate_physics_resource_value(
    physics: &RawPhysicsResource,
    reference: &str,
) -> Result<(), ModelError> {
    if physics.version != 3 {
        return invalid_resource(reference, "physics3 Version must be 3");
    }
    if physics
        .meta
        .fps
        .is_some_and(|fps| !fps.is_finite() || fps <= 0.0)
    {
        return invalid_resource(reference, "physics3 Meta.Fps must be finite and positive");
    }
    validate_physics_vector(
        physics.meta.effective_forces.gravity,
        reference,
        "Meta.EffectiveForces.Gravity",
    )?;
    validate_physics_vector(
        physics.meta.effective_forces.wind,
        reference,
        "Meta.EffectiveForces.Wind",
    )?;

    let mut setting_ids = BTreeSet::new();
    let mut input_count = 0_usize;
    let mut output_count = 0_usize;
    let mut vertex_count = 0_usize;
    for setting in &physics.settings {
        if setting.id.trim().is_empty() || !setting_ids.insert(setting.id.clone()) {
            return invalid_resource(
                reference,
                "physics3 setting Id must be non-empty and unique",
            );
        }
        if setting.inputs.is_empty() || setting.outputs.is_empty() || setting.vertices.len() < 2 {
            return invalid_resource(
                reference,
                "physics3 settings require input, output, and at least two vertices",
            );
        }
        validate_physics_range(setting.normalization.position, reference, "Position")?;
        validate_physics_range(setting.normalization.angle, reference, "Angle")?;

        for input in &setting.inputs {
            if input.source.id.trim().is_empty() {
                return invalid_resource(reference, "physics3 input Source.Id must be non-empty");
            }
            validate_physics_weight(input.weight, reference, "input Weight")?;
        }
        for output in &setting.outputs {
            if output.destination.id.trim().is_empty() {
                return invalid_resource(
                    reference,
                    "physics3 output Destination.Id must be non-empty",
                );
            }
            validate_physics_weight(output.weight, reference, "output Weight")?;
            if !output.scale.is_finite() {
                return invalid_resource(reference, "physics3 output Scale must be finite");
            }
            if output.vertex_index >= setting.vertices.len() {
                return invalid_resource(
                    reference,
                    "physics3 output VertexIndex must reference a setting vertex",
                );
            }
        }
        for vertex in &setting.vertices {
            validate_physics_vector(vertex.position, reference, "vertex Position")?;
            if [
                vertex.mobility,
                vertex.delay,
                vertex.acceleration,
                vertex.radius,
            ]
            .iter()
            .any(|value| !value.is_finite())
            {
                return invalid_resource(reference, "physics3 vertex coefficients must be finite");
            }
        }

        input_count = input_count
            .checked_add(setting.inputs.len())
            .ok_or_else(|| physics_count_overflow(reference))?;
        output_count = output_count
            .checked_add(setting.outputs.len())
            .ok_or_else(|| physics_count_overflow(reference))?;
        vertex_count = vertex_count
            .checked_add(setting.vertices.len())
            .ok_or_else(|| physics_count_overflow(reference))?;
    }

    let mut dictionary_ids = BTreeSet::new();
    for entry in &physics.meta.dictionary {
        if entry.id.trim().is_empty() || !dictionary_ids.insert(entry.id.clone()) {
            return invalid_resource(
                reference,
                "physics3 dictionary Id must be non-empty and unique",
            );
        }
    }
    if dictionary_ids != setting_ids {
        return invalid_resource(reference, "physics3 dictionary Ids must match setting Ids");
    }
    if physics.meta.setting_count != physics.settings.len()
        || physics.meta.input_count != input_count
        || physics.meta.output_count != output_count
        || physics.meta.vertex_count != vertex_count
    {
        return invalid_resource(
            reference,
            "physics3 Meta counts do not match declared arrays",
        );
    }
    Ok(())
}

pub(crate) fn validate_physics_vector(
    vector: RawPhysicsVector,
    reference: &str,
    label: &'static str,
) -> Result<(), ModelError> {
    if !vector.x.is_finite() || !vector.y.is_finite() {
        return invalid_resource(reference, label);
    }
    Ok(())
}

pub(crate) fn validate_physics_range(
    range: RawPhysicsRange,
    reference: &str,
    label: &'static str,
) -> Result<(), ModelError> {
    if !range.minimum.is_finite()
        || !range.default.is_finite()
        || !range.maximum.is_finite()
        || range.minimum > range.default
        || range.default > range.maximum
    {
        return invalid_resource(reference, label);
    }
    Ok(())
}

pub(crate) fn validate_physics_weight(
    weight: f64,
    reference: &str,
    label: &'static str,
) -> Result<(), ModelError> {
    if !weight.is_finite() || !(0.0..=100.0).contains(&weight) {
        return invalid_resource(reference, label);
    }
    Ok(())
}

pub(crate) fn physics_count_overflow(reference: &str) -> ModelError {
    ModelError::new(
        ModelDiagnostic::ModelResourceInvalid,
        Some(reference),
        "physics3 count overflowed",
    )
}

pub(crate) fn validate_model_user_data_resource(
    path: &Path,
    reference: &str,
    maximum_bytes: u64,
    maximum_depth: usize,
) -> Result<(), ModelError> {
    let user_data: RawModelUserDataResource = read_json(
        path,
        reference,
        maximum_bytes,
        maximum_depth,
        ModelDiagnostic::ModelResourceInvalid,
    )?;
    if user_data.version != 3 {
        return invalid_resource(reference, "userdata3 Version must be 3");
    }
    if user_data.meta.entry_count != user_data.entries.len() {
        return invalid_resource(
            reference,
            "userdata3 Meta.UserDataCount does not match entries",
        );
    }

    let mut entry_ids = BTreeSet::new();
    let total_value_bytes = user_data.entries.iter().try_fold(0_usize, |total, entry| {
        if entry.id.trim().is_empty() || !entry_ids.insert((entry.target, entry.id.as_str())) {
            return Err(());
        }
        total.checked_add(entry.value.len()).ok_or(())
    });
    if total_value_bytes.ok() != Some(user_data.meta.total_value_bytes) {
        return invalid_resource(
            reference,
            "userdata3 entries must have unique non-empty Target/Id pairs and matching byte size",
        );
    }
    Ok(())
}

pub(crate) fn invalid_resource(reference: &str, detail: &'static str) -> Result<(), ModelError> {
    Err(ModelError::new(
        ModelDiagnostic::ModelResourceInvalid,
        Some(reference),
        detail,
    ))
}

pub(crate) fn require_identifier(
    value: &str,
    label: &str,
    resource: &str,
) -> Result<(), ModelError> {
    if value.trim().is_empty() {
        return Err(ModelError::new(
            ModelDiagnostic::ModelJsonInvalid,
            Some(resource),
            format!("{label} must not be blank"),
        ));
    }
    Ok(())
}
