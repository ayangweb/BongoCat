use crate::{PlaybackError, PlaybackErrorCode};
use serde::Deserialize;
use std::{collections::BTreeMap, time::Duration};

const DEFAULT_FADE_SECONDS: f32 = 1.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpressionBlendMode {
    Additive,
    Multiply,
    Overwrite,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExpressionParameter {
    pub id: String,
    pub value: f32,
    pub blend: ExpressionBlendMode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExpressionClip {
    fade_in_seconds: f32,
    fade_out_seconds: f32,
    parameters: BTreeMap<String, ExpressionParameter>,
}

#[derive(Clone, Copy, Debug)]
pub struct ExpressionLayer<'a> {
    pub clip: &'a ExpressionClip,
    pub weight: f32,
}

/// Blend one parameter's expression layers over a caller-provided base value.
///
/// The function is deliberately pure: it does not query or mutate Cubism Core,
/// so the live2d owner remains responsible for reading the current parameter
/// and writing the returned value back.
pub fn evaluate_expression_parameter(id: &str, base: f32, layers: &[ExpressionLayer<'_>]) -> f32 {
    let mut overwrite = base;
    let mut additive = 0.0;
    let mut multiply = 1.0;
    for layer in layers {
        let (next_overwrite, next_additive, next_multiply) = match layer.clip.parameter(id) {
            Some(parameter) => match parameter.blend {
                ExpressionBlendMode::Additive => (base, parameter.value, 1.0),
                ExpressionBlendMode::Multiply => (base, 0.0, parameter.value),
                ExpressionBlendMode::Overwrite => (parameter.value, 0.0, 1.0),
            },
            None => (base, 0.0, 1.0),
        };
        overwrite += (next_overwrite - overwrite) * layer.weight;
        additive += (next_additive - additive) * layer.weight;
        multiply += (next_multiply - multiply) * layer.weight;
    }
    (overwrite + additive) * multiply
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExpression {
    #[serde(rename = "Type")]
    kind: String,
    #[serde(rename = "FadeInTime", default)]
    fade_in_seconds: Option<f32>,
    #[serde(rename = "FadeOutTime", default)]
    fade_out_seconds: Option<f32>,
    #[serde(rename = "Parameters")]
    parameters: Vec<RawParameter>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawParameter {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Value")]
    value: f32,
    #[serde(rename = "Blend", default)]
    blend: Option<RawBlendMode>,
}

#[derive(Clone, Copy, Deserialize)]
enum RawBlendMode {
    Add,
    Multiply,
    Overwrite,
}

impl ExpressionClip {
    pub fn from_slice(bytes: &[u8]) -> Result<Self, PlaybackError> {
        let raw: RawExpression = serde_json::from_slice(bytes).map_err(|error| {
            PlaybackError::new(
                PlaybackErrorCode::ExpressionInvalid,
                format!("exp3 JSON is invalid: {error}"),
            )
        })?;
        if raw.kind != "Live2D Expression" {
            return invalid(format!("unsupported expression Type {:?}", raw.kind));
        }

        let fade_in_seconds = raw.fade_in_seconds.unwrap_or(DEFAULT_FADE_SECONDS);
        let fade_out_seconds = raw.fade_out_seconds.unwrap_or(DEFAULT_FADE_SECONDS);
        validate_fade(fade_in_seconds, "FadeInTime")?;
        validate_fade(fade_out_seconds, "FadeOutTime")?;

        let mut parameters = BTreeMap::new();
        for parameter in raw.parameters {
            if parameter.id.trim().is_empty() {
                return invalid("expression parameter Id must not be blank");
            }
            if !parameter.value.is_finite() {
                return invalid(format!(
                    "expression parameter {} Value must be finite",
                    parameter.id
                ));
            }
            let id = parameter.id;
            let entry = ExpressionParameter {
                id: id.clone(),
                value: parameter.value,
                blend: match parameter.blend.unwrap_or(RawBlendMode::Add) {
                    RawBlendMode::Add => ExpressionBlendMode::Additive,
                    RawBlendMode::Multiply => ExpressionBlendMode::Multiply,
                    RawBlendMode::Overwrite => ExpressionBlendMode::Overwrite,
                },
            };
            if parameters.insert(id.clone(), entry).is_some() {
                return invalid(format!("expression parameter {id:?} is duplicated"));
            }
        }

        Ok(Self {
            fade_in_seconds,
            fade_out_seconds,
            parameters,
        })
    }

    pub fn parameters(&self) -> impl Iterator<Item = &ExpressionParameter> {
        self.parameters.values()
    }

    pub(crate) fn parameter(&self, id: &str) -> Option<&ExpressionParameter> {
        self.parameters.get(id)
    }

    pub fn fade_in_duration(&self) -> Duration {
        Duration::from_secs_f32(self.fade_in_seconds)
    }

    pub fn fade_out_duration(&self) -> Duration {
        Duration::from_secs_f32(self.fade_out_seconds)
    }

    pub fn fade_in_weight(&self, elapsed: Duration) -> f32 {
        fade_weight(elapsed.as_secs_f32(), self.fade_in_seconds)
    }

    pub fn fade_out_weight(&self, elapsed: Duration) -> f32 {
        1.0 - fade_weight(elapsed.as_secs_f32(), self.fade_out_seconds)
    }
}

fn fade_weight(elapsed_seconds: f32, duration_seconds: f32) -> f32 {
    if duration_seconds <= 0.0 {
        return 1.0;
    }
    let progress = (elapsed_seconds / duration_seconds).clamp(0.0, 1.0);
    0.5 - 0.5 * (progress * std::f32::consts::PI).cos()
}

fn validate_fade(value: f32, label: &str) -> Result<(), PlaybackError> {
    if !value.is_finite() || value < 0.0 {
        return invalid(format!("{label} must be finite and non-negative"));
    }
    Ok(())
}

fn invalid<T>(detail: impl Into<String>) -> Result<T, PlaybackError> {
    Err(PlaybackError::new(
        PlaybackErrorCode::ExpressionInvalid,
        detail,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_blend_modes_and_defaults() {
        let clip = ExpressionClip::from_slice(
            br#"{
              "Type":"Live2D Expression",
              "Parameters":[
                {"Id":"Add","Value":1.0},
                {"Id":"Multiply","Value":0.5,"Blend":"Multiply"},
                {"Id":"Overwrite","Value":-1.0,"Blend":"Overwrite"}
              ]
            }"#,
        )
        .expect("valid expression");
        assert_eq!(clip.fade_in_duration(), Duration::from_secs(1));
        assert_eq!(clip.fade_out_duration(), Duration::from_secs(1));
        assert_eq!(
            clip.parameters()
                .map(|parameter| parameter.blend)
                .collect::<Vec<_>>(),
            vec![
                ExpressionBlendMode::Additive,
                ExpressionBlendMode::Multiply,
                ExpressionBlendMode::Overwrite,
            ]
        );
    }

    #[test]
    fn blends_expression_layers_without_a_core_parameter_source() {
        let clip = ExpressionClip::from_slice(
            br#"{
              "Type":"Live2D Expression",
              "Parameters":[
                {"Id":"Add","Value":1.0},
                {"Id":"Multiply","Value":0.5,"Blend":"Multiply"},
                {"Id":"Overwrite","Value":-1.0,"Blend":"Overwrite"}
              ]
            }"#,
        )
        .expect("valid expression");
        let layers = [ExpressionLayer {
            clip: &clip,
            weight: 1.0,
        }];
        assert_eq!(evaluate_expression_parameter("Add", 2.0, &layers), 3.0);
        assert_eq!(evaluate_expression_parameter("Multiply", 2.0, &layers), 1.0);
        assert_eq!(
            evaluate_expression_parameter("Overwrite", 2.0, &layers),
            -1.0
        );
        assert_eq!(evaluate_expression_parameter("Missing", 2.0, &layers), 2.0);
    }

    #[test]
    fn applies_sine_fade_weights() {
        let clip = ExpressionClip::from_slice(
            br#"{"Type":"Live2D Expression","FadeInTime":2.0,"FadeOutTime":2.0,"Parameters":[]}"#,
        )
        .expect("valid expression");
        assert_eq!(clip.fade_in_weight(Duration::ZERO), 0.0);
        assert!((clip.fade_in_weight(Duration::from_secs(1)) - 0.5).abs() < 0.0001);
        assert_eq!(clip.fade_in_weight(Duration::from_secs(2)), 1.0);
        assert_eq!(clip.fade_out_weight(Duration::ZERO), 1.0);
        assert!((clip.fade_out_weight(Duration::from_secs(1)) - 0.5).abs() < 0.0001);
        assert_eq!(clip.fade_out_weight(Duration::from_secs(2)), 0.0);
    }

    #[test]
    fn rejects_invalid_type_blend_values_and_duplicates() {
        for invalid_json in [
            r#"{"Type":"Other","Parameters":[]}"#,
            r#"{"Type":"Live2D Expression","Parameters":[{"Id":"P","Value":1,"Blend":"Unknown"}]}"#,
            r#"{"Type":"Live2D Expression","Parameters":[{"Id":"P","Value":1},{"Id":"P","Value":2}]}"#,
            r#"{"Type":"Live2D Expression","FadeInTime":-1,"Parameters":[]}"#,
        ] {
            assert_eq!(
                ExpressionClip::from_slice(invalid_json.as_bytes())
                    .expect_err("invalid expression")
                    .code,
                PlaybackErrorCode::ExpressionInvalid
            );
        }
    }
}
