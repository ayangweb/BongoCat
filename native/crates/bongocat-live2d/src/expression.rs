use crate::{Live2dError, Live2dErrorCode};
use bongocat_model::CommittedModel;
use serde::Deserialize;
use std::{collections::BTreeMap, fs, time::Duration};

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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExpressionApplyStatus {
    pub applied_parameter_count: usize,
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
    pub fn load(model: &CommittedModel, name: &str) -> Result<Self, Live2dError> {
        let resource = model
            .index()
            .expressions
            .iter()
            .find(|resource| resource.name == name)
            .ok_or_else(|| {
                Live2dError::new(
                    Live2dErrorCode::ExpressionNotFound,
                    format!("expression {name:?} is not declared by model3"),
                )
            })?;
        let path = model.root().join(&resource.file);
        let bytes = fs::read(&path).map_err(|error| {
            Live2dError::new(
                Live2dErrorCode::ResourceIo,
                format!("cannot read {}: {error}", path.display()),
            )
        })?;
        Self::from_slice(&bytes).map_err(|mut error| {
            error.detail = format!("{}: {}", path.display(), error.detail);
            error
        })
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, Live2dError> {
        let raw: RawExpression = serde_json::from_slice(bytes).map_err(|error| {
            Live2dError::new(
                Live2dErrorCode::ExpressionInvalid,
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

    #[cfg(any(target_os = "macos", target_os = "windows"))]
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

fn validate_fade(value: f32, label: &str) -> Result<(), Live2dError> {
    if !value.is_finite() || value < 0.0 {
        return invalid(format!("{label} must be finite and non-negative"));
    }
    Ok(())
}

fn invalid<T>(detail: impl Into<String>) -> Result<T, Live2dError> {
    Err(Live2dError::new(Live2dErrorCode::ExpressionInvalid, detail))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
    use std::path::{Path, PathBuf};

    fn repository_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root")
            .to_owned()
    }

    fn preset_model(id: &str) -> CommittedModel {
        PresetModelCatalog::open(
            repository_root().join("native/resources/models"),
            ModelPackageLimits::default(),
        )
        .expect("preset catalog")
        .load(&ModelId::parse(id).expect("model id"))
        .expect("preset model")
    }

    #[test]
    fn parses_every_declared_preset_expression() {
        for model_id in ["standard", "keyboard", "gamepad"] {
            let model = preset_model(model_id);
            for resource in &model.index().expressions {
                ExpressionClip::load(&model, &resource.name).expect("preset expression");
            }
        }
    }

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
                Live2dErrorCode::ExpressionInvalid
            );
        }
    }
}
