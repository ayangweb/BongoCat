//! Bounded, platform-neutral evaluation of the declared Cubism physics3 rig.
//! This is an independent Rust implementation of the pinned R5 behavior
//! contract; it does not call or vendor the Framework runtime.

use crate::core::CoreModel;
use crate::{Live2dError, Live2dErrorCode};
use bongocat_model::{
    PhysicsChannel, PhysicsDefinition, PhysicsOutput, PhysicsRange, PhysicsSetting,
};
use std::collections::{BTreeSet, HashMap};
use std::f32::consts::PI;
use std::time::Duration;

// These are Cubism Framework constants; authored per-model coefficients
// (Mobility, Delay, Acceleration, Radius, and Scale) remain data-driven.
const AIR_RESISTANCE: f32 = 5.0;
const MAX_DELTA_TIME: f32 = 5.0;
const MAX_WEIGHT: f32 = 100.0;
const MOVEMENT_THRESHOLD: f32 = 0.001;
const PHYSICS_STEP_EPSILON: f32 = 0.000001;
const MAX_PHYSICS_STEPS_PER_FRAME: usize = 600;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Vector2 {
    x: f32,
    y: f32,
}

impl Vector2 {
    const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    fn normalized(self) -> Self {
        let length = self.length();
        if length == 0.0 || !length.is_finite() {
            self
        } else {
            Self::new(self.x / length, self.y / length)
        }
    }
}

impl std::ops::Add for Vector2 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl std::ops::Sub for Vector2 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl std::ops::Mul<f32> for Vector2 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl std::ops::Neg for Vector2 {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self::new(-self.x, -self.y)
    }
}

impl std::ops::Div<f32> for Vector2 {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

#[derive(Clone, Copy, Debug)]
struct Particle {
    position: Vector2,
    last_position: Vector2,
    last_gravity: Vector2,
    force: Vector2,
    velocity: Vector2,
    mobility: f32,
    delay: f32,
    acceleration: f32,
    radius: f32,
}

impl Particle {
    fn new(vertex: &bongocat_model::PhysicsVertex, initial_position: Vector2) -> Self {
        Self {
            position: initial_position,
            last_position: initial_position,
            last_gravity: Vector2::new(0.0, 1.0),
            force: Vector2::default(),
            velocity: Vector2::default(),
            mobility: vertex.mobility as f32,
            delay: vertex.delay as f32,
            acceleration: vertex.acceleration as f32,
            radius: vertex.radius as f32,
        }
    }
}

pub(crate) struct PhysicsRuntime {
    definition: PhysicsDefinition,
    particles: Vec<Vec<Particle>>,
    current_outputs: Vec<Vec<f32>>,
    previous_outputs: Vec<Vec<f32>>,
    parameter_ids: BTreeSet<String>,
    parameter_cache: HashMap<String, f32>,
    parameter_input_cache: HashMap<String, f32>,
    cache_initialized: bool,
    current_remain_time: f32,
}

impl PhysicsRuntime {
    pub(crate) fn new(definition: PhysicsDefinition) -> Self {
        let parameter_ids = definition
            .settings
            .iter()
            .flat_map(|setting| {
                setting
                    .inputs
                    .iter()
                    .map(|input| input.parameter_id.clone())
                    .chain(
                        setting
                            .outputs
                            .iter()
                            .map(|output| output.parameter_id.clone()),
                    )
            })
            .collect();
        let mut runtime = Self {
            definition,
            particles: Vec::new(),
            current_outputs: Vec::new(),
            previous_outputs: Vec::new(),
            parameter_ids,
            parameter_cache: HashMap::new(),
            parameter_input_cache: HashMap::new(),
            cache_initialized: false,
            current_remain_time: 0.0,
        };
        runtime.reset();
        runtime
    }

    pub(crate) fn reset(&mut self) {
        self.particles.clear();
        self.current_outputs.clear();
        self.previous_outputs.clear();
        for setting in &self.definition.settings {
            let mut particles: Vec<Particle> = Vec::with_capacity(setting.vertices.len());
            for (index, vertex) in setting.vertices.iter().enumerate() {
                let initial_position = if index == 0 {
                    Vector2::default()
                } else {
                    particles[index - 1].position + Vector2::new(0.0, vertex.radius as f32)
                };
                particles.push(Particle::new(vertex, initial_position));
            }
            self.particles.push(particles);
            self.current_outputs.push(vec![0.0; setting.outputs.len()]);
            self.previous_outputs.push(vec![0.0; setting.outputs.len()]);
        }
        self.parameter_cache.clear();
        self.parameter_input_cache.clear();
        self.cache_initialized = false;
        self.current_remain_time = 0.0;
    }

    pub(crate) fn evaluate(
        &mut self,
        delta: Duration,
        core: &mut CoreModel,
    ) -> Result<usize, Live2dError> {
        let delta = delta.as_secs_f32();
        if !delta.is_finite() || delta <= 0.0 {
            return Ok(0);
        }
        self.ensure_cache(core)?;
        self.current_remain_time += delta;
        if self.current_remain_time > MAX_DELTA_TIME {
            self.current_remain_time = 0.0;
        }

        let physics_delta = if self.definition.fps > 0.0 {
            1.0 / self.definition.fps as f32
        } else {
            delta
        };
        if !physics_delta.is_finite() || physics_delta <= 0.0 {
            return Err(Live2dError::new(
                Live2dErrorCode::ParameterValueInvalid,
                "physics3 has an invalid evaluation step",
            ));
        }

        let mut applied = 0;
        let mut steps = 0_usize;
        while self.current_remain_time + PHYSICS_STEP_EPSILON >= physics_delta {
            if steps >= MAX_PHYSICS_STEPS_PER_FRAME {
                return Err(Live2dError::new(
                    Live2dErrorCode::ParameterValueInvalid,
                    "physics3 evaluation exceeded its bounded step budget",
                ));
            }
            self.step(core, physics_delta)?;
            self.current_remain_time = (self.current_remain_time - physics_delta).max(0.0);
            steps += 1;
        }
        let alpha = (self.current_remain_time / physics_delta).clamp(0.0, 1.0);
        applied += self.interpolate(core, alpha)?;
        Ok(applied)
    }

    fn ensure_cache(&mut self, core: &CoreModel) -> Result<(), Live2dError> {
        if self.cache_initialized {
            return Ok(());
        }
        for id in &self.parameter_ids {
            if let Some(value) = core.parameter_value_by_id(id)? {
                self.parameter_cache.insert(id.clone(), value);
                self.parameter_input_cache.insert(id.to_owned(), value);
            }
        }
        self.cache_initialized = true;
        Ok(())
    }

    fn step(&mut self, core: &CoreModel, physics_delta: f32) -> Result<(), Live2dError> {
        for setting_index in 0..self.definition.settings.len() {
            for output_index in 0..self.current_outputs[setting_index].len() {
                self.previous_outputs[setting_index][output_index] =
                    self.current_outputs[setting_index][output_index];
            }
        }

        let input_weight = if self.current_remain_time > 0.0 {
            physics_delta / self.current_remain_time
        } else {
            1.0
        };
        for id in &self.parameter_ids {
            let Some(actual) = core.parameter_value_by_id(id)? else {
                continue;
            };
            let previous = self
                .parameter_input_cache
                .get(id)
                .copied()
                .unwrap_or(actual);
            let value = previous + (actual - previous) * input_weight;
            self.parameter_cache.insert(id.clone(), value);
            self.parameter_input_cache.insert(id.to_owned(), value);
        }

        for setting_index in 0..self.definition.settings.len() {
            let setting = self.definition.settings[setting_index].clone();
            let (initial_translation, total_angle) = self.input_for_setting(core, &setting)?;
            // `total_angle` is expressed in degrees; convert exactly once
            // before applying the sub-rig rotation.
            let radian = rotation_radians(total_angle);
            let rotated_x =
                initial_translation.x * radian.cos() - initial_translation.y * radian.sin();
            let rotated_y = rotated_x * radian.sin() + initial_translation.y * radian.cos();
            let translation = Vector2::new(rotated_x, rotated_y);

            self.update_particles(
                setting_index,
                &setting,
                translation,
                total_angle,
                physics_delta,
            );

            for (output_index, output) in setting.outputs.iter().enumerate() {
                if !output_index_is_valid(output, self.particles[setting_index].len()) {
                    continue;
                }
                let value = self.output_value(output, &self.particles[setting_index]);
                if !value.is_finite() {
                    return Err(Live2dError::new(
                        Live2dErrorCode::ParameterValueInvalid,
                        "physics3 produced a non-finite output",
                    ));
                }
                self.current_outputs[setting_index][output_index] = value;
                self.update_cached_output(core, output, value)?;
            }
        }
        Ok(())
    }

    fn input_for_setting(
        &self,
        core: &CoreModel,
        setting: &PhysicsSetting,
    ) -> Result<(Vector2, f32), Live2dError> {
        let mut translation = Vector2::default();
        let mut total_angle = 0.0;
        for input in &setting.inputs {
            let Some(value) = self.parameter_cache.get(&input.parameter_id).copied() else {
                continue;
            };
            let Some(parameter_range) = core.parameter_range_by_id(&input.parameter_id) else {
                continue;
            };
            let normalized = normalize_parameter(
                value,
                parameter_range.minimum,
                parameter_range.maximum,
                parameter_range.default,
                match input.channel {
                    PhysicsChannel::X | PhysicsChannel::Y => setting.normalization_position,
                    PhysicsChannel::Angle => setting.normalization_angle,
                },
                input.reflect,
            );
            let normalized = normalized * input.weight as f32 / MAX_WEIGHT;
            match input.channel {
                PhysicsChannel::X => translation.x += normalized,
                PhysicsChannel::Y => translation.y += normalized,
                PhysicsChannel::Angle => total_angle += normalized,
            }
        }
        Ok((translation, total_angle))
    }

    fn update_particles(
        &mut self,
        setting_index: usize,
        setting: &PhysicsSetting,
        total_translation: Vector2,
        total_angle: f32,
        delta: f32,
    ) {
        let particles = &mut self.particles[setting_index];
        let wind = Vector2::new(self.definition.wind.x as f32, self.definition.wind.y as f32);
        let current_gravity = Vector2::new(
            (total_angle * PI / 180.0).sin(),
            (total_angle * PI / 180.0).cos(),
        )
        .normalized();
        particles[0].position = total_translation;
        let threshold = MOVEMENT_THRESHOLD * setting.normalization_position.maximum as f32;

        for index in 1..particles.len() {
            let acceleration = particles[index].acceleration;
            particles[index].force = Vector2::new(
                current_gravity.x * acceleration + wind.x,
                current_gravity.y * acceleration + wind.y,
            );
            particles[index].last_position = particles[index].position;
            let delay = particles[index].delay * delta * 30.0;
            let mut direction = particles[index].position - particles[index - 1].position;
            let rotation = direction_to_radian(particles[index].last_gravity, current_gravity)
                / AIR_RESISTANCE;
            let rotated_x = direction.x * rotation.cos() - direction.y * rotation.sin();
            let rotated_y = rotated_x * rotation.sin() + direction.y * rotation.cos();
            direction = Vector2::new(rotated_x, rotated_y);
            particles[index].position = particles[index - 1].position + direction;
            let velocity = particles[index].velocity * delay;
            let force = particles[index].force * (delay * delay);
            particles[index].position = particles[index].position + velocity + force;
            let new_direction =
                (particles[index].position - particles[index - 1].position).normalized();
            particles[index].position =
                particles[index - 1].position + new_direction * particles[index].radius;
            if particles[index].position.x.abs() < threshold {
                particles[index].position.x = 0.0;
            }
            if delay != 0.0 {
                particles[index].velocity = Vector2::new(
                    particles[index].position.x - particles[index].last_position.x,
                    particles[index].position.y - particles[index].last_position.y,
                ) / delay
                    * particles[index].mobility;
            }
            particles[index].force = Vector2::default();
            particles[index].last_gravity = current_gravity;
        }
    }

    fn output_value(&self, output: &PhysicsOutput, particles: &[Particle]) -> f32 {
        let index = output.vertex_index;
        if index == 0 || index >= particles.len() {
            return 0.0;
        }
        let translation = particles[index].position - particles[index - 1].position;
        let value = match output.channel {
            PhysicsChannel::X => translation.x,
            PhysicsChannel::Y => translation.y,
            PhysicsChannel::Angle => {
                let parent_gravity = if index >= 2 {
                    particles[index - 1].position - particles[index - 2].position
                } else {
                    -Vector2::new(
                        self.definition.gravity.x as f32,
                        self.definition.gravity.y as f32,
                    )
                };
                direction_to_radian(parent_gravity, translation)
            }
        };
        let value = value * output.scale as f32;
        if output.reflect { -value } else { value }
    }

    fn update_cached_output(
        &mut self,
        core: &CoreModel,
        output: &PhysicsOutput,
        value: f32,
    ) -> Result<(), Live2dError> {
        let Some(range) = core.parameter_range_by_id(&output.parameter_id) else {
            return Ok(());
        };
        let current = self
            .parameter_cache
            .get(&output.parameter_id)
            .copied()
            .or(core.parameter_value_by_id(&output.parameter_id)?)
            .unwrap_or(range.default);
        let weight = (output.weight as f32 / MAX_WEIGHT).clamp(0.0, 1.0);
        let next = blend_output_value(current, value, range, weight);
        self.parameter_cache
            .insert(output.parameter_id.clone(), next);
        Ok(())
    }

    fn interpolate(&mut self, core: &mut CoreModel, alpha: f32) -> Result<usize, Live2dError> {
        let mut applied = 0;
        for setting_index in 0..self.definition.settings.len() {
            let setting = &self.definition.settings[setting_index];
            for (output_index, output) in setting.outputs.iter().enumerate() {
                if !output_index_is_valid(output, self.particles[setting_index].len()) {
                    continue;
                }
                let Some(range) = core.parameter_range_by_id(&output.parameter_id) else {
                    continue;
                };
                let value = self.previous_outputs[setting_index][output_index] * (1.0 - alpha)
                    + self.current_outputs[setting_index][output_index] * alpha;
                // Cubism clamps the raw physics result before applying the
                // output weight. Clamping only after the blend makes a partial
                // output overshoot the authored range and then get pinned to
                // the boundary, which visibly stiffens secondary hair motion.
                let value = value.clamp(range.minimum, range.maximum);
                let weight = (output.weight as f32 / MAX_WEIGHT).clamp(0.0, 1.0);
                if matches!(
                    core.set_parameter_by_id(&output.parameter_id, value, weight)?,
                    crate::ParameterUpdate::Applied { .. }
                ) {
                    applied += 1;
                }
            }
        }
        Ok(applied)
    }
}

fn rotation_radians(total_angle: f32) -> f32 {
    -total_angle * PI / 180.0
}

fn output_index_is_valid(output: &PhysicsOutput, particle_count: usize) -> bool {
    output.vertex_index > 0 && output.vertex_index < particle_count
}

fn blend_output_value(current: f32, raw: f32, range: crate::ParameterRange, weight: f32) -> f32 {
    let bounded = raw.clamp(range.minimum, range.maximum);
    if weight >= 1.0 {
        bounded
    } else {
        current + (bounded - current) * weight
    }
}

fn normalize_parameter(
    value: f32,
    parameter_minimum: f32,
    parameter_maximum: f32,
    _parameter_default: f32,
    normalization: PhysicsRange,
    reflected: bool,
) -> f32 {
    let maximum = parameter_maximum.max(parameter_minimum);
    let minimum = parameter_minimum.min(parameter_maximum);
    let value = value.clamp(minimum, maximum);
    let middle = (minimum + maximum) * 0.5;
    let normalized_minimum = normalization.minimum.min(normalization.maximum) as f32;
    let normalized_maximum = normalization.minimum.max(normalization.maximum) as f32;
    let normalized_default = normalization.default as f32;
    let parameter_delta = value - middle;
    let result = if parameter_delta > 0.0 {
        let normalized_length = normalized_maximum - normalized_default;
        let parameter_length = maximum - middle;
        if parameter_length == 0.0 {
            0.0
        } else {
            parameter_delta * normalized_length / parameter_length + normalized_default
        }
    } else if parameter_delta < 0.0 {
        let normalized_length = normalized_minimum - normalized_default;
        let parameter_length = minimum - middle;
        if parameter_length == 0.0 {
            0.0
        } else {
            parameter_delta * normalized_length / parameter_length + normalized_default
        }
    } else {
        normalized_default
    };
    if reflected { result } else { -result }
}

fn direction_to_radian(from: Vector2, to: Vector2) -> f32 {
    let mut result = to.y.atan2(to.x) - from.y.atan2(from.x);
    while result < -PI {
        result += PI * 2.0;
    }
    while result > PI {
        result -= PI * 2.0;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subrig_angles_are_converted_to_radians_exactly_once() {
        assert!((rotation_radians(30.0) + PI / 6.0).abs() < 1.0e-6);
        assert!((rotation_radians(90.0) + PI / 2.0).abs() < 1.0e-6);
        assert!((rotation_radians(-45.0) - PI / 4.0).abs() < 1.0e-6);
    }

    #[test]
    fn output_values_are_bounded_before_the_weight_is_applied() {
        let range = crate::ParameterRange {
            minimum: 0.0,
            maximum: 1.0,
            default: 0.0,
        };
        assert_eq!(blend_output_value(0.5, 2.0, range, 0.5), 0.75);
        assert_eq!(blend_output_value(0.5, -2.0, range, 0.5), 0.25);
        assert_eq!(blend_output_value(0.5, 2.0, range, 1.0), 1.0);
    }

    #[test]
    fn the_root_particle_is_not_a_valid_physics_output() {
        let output = PhysicsOutput {
            parameter_id: "Param".to_owned(),
            vertex_index: 0,
            scale: 1.0,
            weight: 100.0,
            channel: PhysicsChannel::Angle,
            reflect: false,
        };
        assert!(!output_index_is_valid(&output, 2));
        assert!(!output_index_is_valid(
            &PhysicsOutput {
                vertex_index: 2,
                ..output.clone()
            },
            2
        ));
        assert!(output_index_is_valid(
            &PhysicsOutput {
                vertex_index: 1,
                ..output
            },
            2
        ));
    }

    #[test]
    fn reflected_input_normalization_keeps_the_authored_direction() {
        let normalization = PhysicsRange {
            minimum: -10.0,
            default: 0.0,
            maximum: 10.0,
        };
        assert_eq!(
            normalize_parameter(30.0, -30.0, 30.0, 0.0, normalization, false),
            -10.0
        );
        assert_eq!(
            normalize_parameter(30.0, -30.0, 30.0, 0.0, normalization, true),
            10.0
        );
    }
}
