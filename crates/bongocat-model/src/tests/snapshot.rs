//! What a frame exposes, and what it withholds.

use super::*;

#[test]
fn model_snapshot_exposes_only_declared_behavior_identities() {
    let catalog = PresetModelCatalog::open(
        repository_root().join("resources/models"),
        ModelPackageLimits::default(),
    )
    .expect("preset catalog");
    let snapshot = catalog
        .load(&ModelId::parse("standard").expect("model id"))
        .expect("committed preset")
        .snapshot();

    assert_eq!(snapshot.behaviors.len(), 7);
    assert!(snapshot.behaviors.contains(&ModelBehaviorSnapshot::Motion {
        group: "CAT_motion".to_owned(),
        index: 0,
    }));
    assert!(
        snapshot
            .behaviors
            .contains(&ModelBehaviorSnapshot::Expression {
                name: "live2d_expression2.exp3.json".to_owned(),
            })
    );
    assert!(snapshot.behaviors.iter().all(|behavior| match behavior {
        ModelBehaviorSnapshot::Motion { group, .. } =>
            !group.contains('/') && !group.contains('\\'),
        ModelBehaviorSnapshot::Expression { name } => !name.contains('/') && !name.contains('\\'),
    }));
}
