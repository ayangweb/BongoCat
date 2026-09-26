use crate::{NativeConfig, WindowState};
use schemars::{JsonSchema, SchemaGenerator, generate::SchemaSettings};
use serde_json::{Map, Value};
use std::{fs, io, path::Path};

const CONFIG_SCHEMA_FILE: &str = "config.schema.json";
const WINDOW_STATE_SCHEMA_FILE: &str = "window-state.schema.json";

/// Writes the checked-in JSON Schema documents used by the independent fixture
/// and tooling gates. This is a developer entry point, not a runtime writer.
#[doc(hidden)]
pub fn write_json_schemas(directory: &Path) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    for (file_name, schema) in [
        (CONFIG_SCHEMA_FILE, configuration_schema()),
        (WINDOW_STATE_SCHEMA_FILE, window_state_schema()),
    ] {
        let mut bytes = serde_json::to_vec_pretty(&schema).map_err(|error| {
            io::Error::other(format!("JSON schema serialization failed: {error}"))
        })?;
        bytes.push(b'\n');
        fs::write(directory.join(file_name), bytes)?;
    }
    Ok(())
}

fn configuration_schema() -> Value {
    let mut schema = generated_schema::<NativeConfig>();
    remove_descriptions(&mut schema);
    let root = schema.as_object_mut().expect("root schema is an object");
    set_document_metadata(
        root,
        "https://bongocat.dev/schemas/config-v1.json",
        "BongoCat configuration",
    );
    set_root_constant(root, "schema_version", 1);
    require_definition_property(root, "ModelConfig", "selected_model");
    set_exclusive_maximum(root, "GamepadInputConfig", "stick_dead_zone");
    set_exclusive_maximum(root, "GamepadInputConfig", "trigger_dead_zone");
    set_unique_items(root, "ModelConfig", "imported_models");
    set_unique_items(root, "ModelConfig", "built_in_models");
    schema
}

fn window_state_schema() -> Value {
    let mut schema = generated_schema::<WindowState>();
    remove_descriptions(&mut schema);
    let root = schema.as_object_mut().expect("root schema is an object");
    set_document_metadata(
        root,
        "https://ayangweb.com/bongocat/window-state-v1.schema.json",
        "BongoCat Window State v1",
    );
    set_root_constant(root, "schema_version", 1);
    require_root_property(root, "settings_window");
    require_root_property(root, "overlay_window");
    schema
}

fn generated_schema<T: JsonSchema>() -> Value {
    serde_json::to_value(
        SchemaGenerator::new(SchemaSettings::draft2020_12()).into_root_schema_for::<T>(),
    )
    .expect("schemars output is JSON serializable")
}

fn set_document_metadata(root: &mut Map<String, Value>, id: &str, title: &str) {
    root.insert(
        "$schema".to_owned(),
        Value::String("https://json-schema.org/draft/2020-12/schema".to_owned()),
    );
    root.insert("$id".to_owned(), Value::String(id.to_owned()));
    root.insert("title".to_owned(), Value::String(title.to_owned()));
}

fn set_root_constant(root: &mut Map<String, Value>, property: &str, value: u64) {
    let property = root
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .and_then(|properties| properties.get_mut(property))
        .and_then(Value::as_object_mut)
        .expect("root schema contains its version property");
    property.clear();
    property.insert("const".to_owned(), Value::from(value));
}

fn require_root_property(root: &mut Map<String, Value>, property: &str) {
    let required = root
        .get_mut("required")
        .and_then(Value::as_array_mut)
        .expect("root schema has a required list");
    if !required.iter().any(|value| value == property) {
        required.push(Value::String(property.to_owned()));
    }
}

fn require_definition_property(root: &mut Map<String, Value>, definition: &str, property: &str) {
    let required = definition_mut(root, definition)
        .get_mut("required")
        .and_then(Value::as_array_mut)
        .expect("schema definition has a required list");
    if !required.iter().any(|value| value == property) {
        required.push(Value::String(property.to_owned()));
    }
}

fn set_exclusive_maximum(root: &mut Map<String, Value>, definition: &str, property: &str) {
    let property = property_mut(definition_mut(root, definition), property);
    property.remove("maximum");
    property.insert("exclusiveMaximum".to_owned(), Value::from(1.0));
}

fn set_unique_items(root: &mut Map<String, Value>, definition: &str, property: &str) {
    property_mut(definition_mut(root, definition), property)
        .insert("uniqueItems".to_owned(), Value::Bool(true));
}

fn definition_mut<'a>(root: &'a mut Map<String, Value>, definition: &str) -> &'a mut Value {
    root.get_mut("$defs")
        .and_then(Value::as_object_mut)
        .and_then(|definitions| definitions.get_mut(definition))
        .unwrap_or_else(|| panic!("generated schema contains definition {definition}"))
}

fn property_mut<'a>(definition: &'a mut Value, property: &str) -> &'a mut Map<String, Value> {
    definition
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .and_then(|properties| properties.get_mut(property))
        .and_then(Value::as_object_mut)
        .unwrap_or_else(|| panic!("generated schema contains property {property}"))
}

fn remove_descriptions(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("description");
            for value in object.values_mut() {
                remove_descriptions(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                remove_descriptions(value);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{CONFIG_SCHEMA_FILE, WINDOW_STATE_SCHEMA_FILE, write_json_schemas};
    use std::{fs, path::Path};
    use tempfile::tempdir;

    #[test]
    fn checked_in_json_schemas_match_the_rust_types() {
        let checked_in = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../shared/config");
        let generated = tempdir().expect("generated schema directory");
        write_json_schemas(generated.path()).expect("generate JSON schemas");

        for file_name in [CONFIG_SCHEMA_FILE, WINDOW_STATE_SCHEMA_FILE] {
            let expected = serde_json::from_slice::<serde_json::Value>(
                &fs::read(checked_in.join(file_name)).expect("checked-in schema"),
            )
            .expect("checked-in schema JSON");
            let actual = serde_json::from_slice::<serde_json::Value>(
                &fs::read(generated.path().join(file_name)).expect("generated schema"),
            )
            .expect("generated schema JSON");
            assert_eq!(actual, expected, "generated schema drifted: {file_name}");
        }
    }
}
