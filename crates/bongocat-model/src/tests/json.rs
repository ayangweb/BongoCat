//! The nesting limit, applied before anything is deserialized.

use super::*;

#[test]
fn json_nesting_limit_is_applied_before_typed_deserialization() {
    let package = tempdir().expect("package");
    let nesting = "[".repeat(65) + &"]".repeat(65);
    fs::write(
        package.path().join("cat.model3.json"),
        format!(
            r#"{{"Version":3,"FileReferences":{{"Moc":"model.moc3","Textures":[]}},"Nested":{nesting}}}"#
        ),
    )
    .expect("model3");
    fs::write(package.path().join("model.moc3"), b"moc").expect("moc");

    let error = PreparedModel::prepare(
        ModelId::parse("deep").expect("model id"),
        package.path(),
        ModelPackageLimits::default(),
    )
    .expect_err("deep JSON must be rejected");
    assert_eq!(error.code, ModelDiagnostic::ModelJsonInvalid);
    assert!(error.detail.contains("nesting"));
}

proptest! {


    #[test]
    fn arbitrary_model_json_bytes_never_escape_the_bounded_parser(
        bytes in proptest::collection::vec(any::<u8>(), 0..8_192),
        maximum_depth in 1_usize..=64,
    ) {
        let result: Result<ModelDefinition, ModelError> = parse_json_bytes(
            &bytes,
            "generated.model3.json",
            maximum_depth,
            ModelDiagnostic::ModelJsonInvalid,
        );
        if let Err(error) = result {
            prop_assert_eq!(error.code, ModelDiagnostic::ModelJsonInvalid);
            prop_assert_eq!(error.resource.as_deref(), Some("generated.model3.json"));
        }
    }

    #[test]
    fn generated_model_array_indices_round_trip_without_loss(
        textures in proptest::collection::vec(any::<String>(), 0..64),
        group_ids in proptest::collection::vec(any::<String>(), 0..64),
    ) {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "Version": 3,
            "FileReferences": {
                "Moc": "model.moc3",
                "Textures": textures,
            },
            "Groups": [{
                "Target": "Parameter",
                "Name": "Generated",
                "Ids": group_ids,
            }],
        }))
        .expect("generated model JSON");
        let parsed: ModelDefinition = parse_json_bytes(
            &bytes,
            "generated.model3.json",
            16,
            ModelDiagnostic::ModelJsonInvalid,
        )?;

        prop_assert_eq!(parsed.files.textures, textures);
        prop_assert_eq!(parsed.groups.len(), 1);
        prop_assert_eq!(&parsed.groups[0].ids, &group_ids);
    }
}
