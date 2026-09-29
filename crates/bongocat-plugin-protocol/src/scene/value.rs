//! The values a scene binds to, and the shapes those values take once resolved.
//!
//! A scene never computes anything. It names a path, and the host writes the
//! value at that path each time the plugin's state changes. The indirection is
//! the whole point: a countdown's seconds are produced by a behavior, not by the
//! text node that shows them, so a plugin can rearrange its panel without
//! touching its logic and the logic cannot be tricked into drawing more than the
//! host decided it should.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A value a scene node reads.
///
/// The wire form is hand-written rather than derived, and it is worth saying why
/// because the derived form is worse: a bare JSON string is a literal, and the
/// three bound forms are objects. A derived externally-tagged enum would write
/// the literal as `{"text": "Focus"}`, so the most common case in a scene — a
/// label that never changes — would cost a wrapper nobody wants to read or
/// write. A plugin author writing a panel should be able to write `"Focus"` and
/// mean it.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneValue {
    /// A constant. The common case for labels that never change.
    Text(String),
    /// A host value, as text. `fallback` is what shows when the path is not in
    /// this frame's state, which is what a panel shows for the instant between a
    /// behavior being removed and the scene being rebuilt.
    Binding { binding: String, fallback: String },
    /// A host value, as a proportion in `[0, 1]`. Out-of-range values clamp
    /// rather than being refused, because a value that raced a resize should not
    /// be able to empty a panel.
    Fraction { binding: String, fallback: f32 },
    /// A host value, as a yes or no. Anything but an exact `1.0` is no.
    Flag { binding: String, fallback: bool },
}

impl Default for SceneValue {
    /// An empty literal.
    ///
    /// A node whose value is left out is a label that has nothing to say yet, and
    /// the empty string is the one answer that cannot be wrong about a panel the
    /// host has not heard from.
    fn default() -> Self {
        Self::Text(String::new())
    }
}

impl Serialize for SceneValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            Self::Text(text) => serializer.serialize_str(text),
            Self::Binding { binding, fallback } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("binding", binding)?;
                map.serialize_entry("fallback", fallback)?;
                map.end()
            }
            Self::Fraction { binding, fallback } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("fraction", binding)?;
                map.serialize_entry("fallback", fallback)?;
                map.end()
            }
            Self::Flag { binding, fallback } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("flag", binding)?;
                map.serialize_entry("fallback", fallback)?;
                map.end()
            }
        }
    }
}

/// Which of the three bound forms a value object named.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ValueKind {
    Text,
    Fraction,
    Flag,
}

impl ValueKind {
    fn parse(key: &str) -> Option<Self> {
        match key {
            "binding" => Some(Self::Text),
            "fraction" => Some(Self::Fraction),
            "flag" => Some(Self::Flag),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for SceneValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ValueVisitor;

        impl<'de> serde::de::Visitor<'de> for ValueVisitor {
            type Value = SceneValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(
                    "a string, or an object with one of `binding`, `fraction` or `flag` and a `fallback`",
                )
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(SceneValue::Text(value.to_string()))
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut binding: Option<String> = None;
                let mut kind: Option<ValueKind> = None;
                let mut fallback: Option<serde_json::Value> = None;
                while let Some(key) = map.next_key::<String>()? {
                    match ValueKind::parse(&key) {
                        Some(found) => {
                            if kind.replace(found).is_some() {
                                return Err(serde::de::Error::custom(
                                    "a value names exactly one of `binding`, `fraction` or `flag`",
                                ));
                            }
                            binding = Some(map.next_value()?);
                        }
                        _ if key == "fallback" => fallback = Some(map.next_value()?),
                        _ => {
                            return Err(serde::de::Error::custom(format!(
                                "unknown value field `{key}`; expected one of `binding`, `fraction`, `flag`, `fallback`"
                            )));
                        }
                    }
                }
                let (kind, binding) = kind.zip(binding).ok_or_else(|| {
                    serde::de::Error::custom(
                        "a value object names one of `binding`, `fraction` or `flag`",
                    )
                })?;
                // Each kind reads its own fallback type, so a `fallback` that does
                // not match is a type error at the point it was written rather
                // than a silently substituted default.
                Ok(match kind {
                    ValueKind::Text => SceneValue::Binding {
                        binding,
                        fallback: match fallback {
                            Some(serde_json::Value::String(text)) => text,
                            _ => {
                                return Err(serde::de::Error::custom(
                                    "a `binding` value takes a string `fallback`",
                                ));
                            }
                        },
                    },
                    ValueKind::Fraction => SceneValue::Fraction {
                        binding,
                        fallback: match fallback {
                            Some(serde_json::Value::Number(number)) => {
                                number.as_f64().ok_or_else(|| {
                                    serde::de::Error::custom("`fallback` is not a number")
                                })? as f32
                            }
                            _ => {
                                return Err(serde::de::Error::custom(
                                    "a `fraction` value takes a number `fallback`",
                                ));
                            }
                        },
                    },
                    ValueKind::Flag => SceneValue::Flag {
                        binding,
                        fallback: match fallback {
                            Some(serde_json::Value::Bool(flag)) => flag,
                            _ => {
                                return Err(serde::de::Error::custom(
                                    "a `flag` value takes a boolean `fallback`",
                                ));
                            }
                        },
                    },
                })
            }
        }

        deserializer.deserialize_any(ValueVisitor)
    }
}

/// One resolved value.
#[derive(Clone, Debug, PartialEq)]
pub enum BindingValue {
    Text(String),
    Fraction(f32),
    Flag(bool),
}

impl BindingValue {
    /// Read a value as text, for a node that asked for text.
    ///
    /// A fraction renders as a percentage rather than as its raw decimal: a bar
    /// bound to `0.42` shown as `0.42` reads as a bug in a panel, and the panel
    /// is not the place to teach what a fraction is.
    pub fn as_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Fraction(fraction) => {
                format!("{}%", (fraction.clamp(0.0, 1.0) * 100.0).round() as i64)
            }
            Self::Flag(flag) => {
                if *flag {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
        }
    }

    /// Read a value as a proportion, for a node that asked for one.
    pub fn as_fraction(&self) -> f32 {
        match self {
            Self::Fraction(fraction) => fraction.clamp(0.0, 1.0),
            Self::Flag(flag) => f32::from(*flag),
            Self::Text(_) => 0.0,
        }
    }

    /// Read a value as a yes or no, for a node that asked for one.
    pub fn as_flag(&self) -> bool {
        match self {
            Self::Flag(flag) => *flag,
            Self::Fraction(fraction) => *fraction >= 1.0,
            Self::Text(text) => !text.is_empty(),
        }
    }
}

/// Everything a scene can read this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BindingTable {
    values: BTreeMap<String, BindingValue>,
}

impl BindingTable {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn set(&mut self, path: impl Into<String>, value: BindingValue) {
        self.values.insert(path.into(), value);
    }

    pub fn get(&self, path: &str) -> Option<&BindingValue> {
        self.values.get(path)
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Resolve a value against this table, falling back when the path is absent.
    pub fn resolve(&self, value: &SceneValue) -> ResolvedValue {
        match value {
            SceneValue::Text(text) => ResolvedValue::Text(text.clone()),
            SceneValue::Binding { binding, fallback } => ResolvedValue::Text(
                self.values
                    .get(binding)
                    .map_or_else(|| fallback.clone(), BindingValue::as_text),
            ),
            SceneValue::Fraction { binding, fallback } => ResolvedValue::Fraction(
                self.values
                    .get(binding)
                    .map_or(*fallback, BindingValue::as_fraction)
                    .clamp(0.0, 1.0),
            ),
            SceneValue::Flag { binding, fallback } => ResolvedValue::Flag(
                self.values
                    .get(binding)
                    .map_or(*fallback, BindingValue::as_flag),
            ),
        }
    }
}

/// A [`SceneValue`] after resolution.
#[derive(Clone, Debug, PartialEq)]
pub enum ResolvedValue {
    Text(String),
    Fraction(f32),
    Flag(bool),
}

impl ResolvedValue {
    pub fn as_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Fraction(fraction) => {
                format!("{}%", (fraction.clamp(0.0, 1.0) * 100.0).round() as i64)
            }
            Self::Flag(flag) => {
                if *flag {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
        }
    }

    pub fn as_fraction(&self) -> f32 {
        match self {
            Self::Fraction(fraction) => fraction.clamp(0.0, 1.0),
            Self::Flag(flag) => f32::from(*flag),
            Self::Text(_) => 0.0,
        }
    }

    pub fn as_flag(&self) -> bool {
        match self {
            Self::Flag(flag) => *flag,
            Self::Fraction(fraction) => *fraction >= 1.0,
            Self::Text(text) => !text.is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> BindingTable {
        let mut table = BindingTable::new();
        table.set("timer.progress", BindingValue::Fraction(0.25));
        table.set("timer.running", BindingValue::Flag(true));
        table.set("timer.label", BindingValue::Text("25:00".to_string()));
        table
    }

    #[test]
    fn a_literal_ignores_the_table_entirely() {
        assert_eq!(
            table().resolve(&SceneValue::Text("Focus".to_string())),
            ResolvedValue::Text("Focus".to_string())
        );
    }

    #[test]
    fn a_missing_path_uses_the_fallback_rather_than_an_empty_value() {
        let value = SceneValue::Binding {
            binding: "timer.absent".to_string(),
            fallback: "--".to_string(),
        };
        assert_eq!(
            table().resolve(&value),
            ResolvedValue::Text("--".to_string())
        );
    }

    #[test]
    fn a_fraction_shows_as_a_percentage_when_read_as_text() {
        let value = SceneValue::Binding {
            binding: "timer.progress".to_string(),
            fallback: "0".to_string(),
        };
        assert_eq!(table().resolve(&value).as_text(), "25%");
    }

    #[test]
    fn an_out_of_range_fraction_clamps_on_the_way_in_and_out() {
        let mut table = BindingTable::new();
        table.set("over", BindingValue::Fraction(4.0));
        table.set("under", BindingValue::Fraction(-2.0));
        assert_eq!(table.get("over").unwrap().as_fraction(), 1.0);
        assert_eq!(table.get("under").unwrap().as_fraction(), 0.0);
        let value = SceneValue::Fraction {
            binding: "over".to_string(),
            fallback: 0.0,
        };
        assert_eq!(table.resolve(&value).as_fraction(), 1.0);
    }

    #[test]
    fn reading_a_value_as_the_wrong_shape_does_not_panic() {
        assert_eq!(BindingValue::Text("x".to_string()).as_fraction(), 0.0);
        assert!(!BindingValue::Text(String::new()).as_flag());
        assert!(BindingValue::Text("x".to_string()).as_flag());
        assert!(BindingValue::Fraction(1.0).as_flag());
        assert!(!BindingValue::Fraction(0.99).as_flag());
    }

    #[test]
    fn the_written_form_uses_the_snake_case_kind() {
        let value: SceneValue =
            serde_json::from_str(r#"{"fraction":"t.p","fallback":0.5}"#).unwrap();
        assert!(matches!(value, SceneValue::Fraction { .. }));
        let flag: SceneValue = serde_json::from_str(r#"{"flag":"t.r","fallback":true}"#).unwrap();
        assert!(matches!(flag, SceneValue::Flag { .. }));
    }
}
