//! Local investigation probe (not part of the product): locate known artwork
//! pixels (by texture UV) in model space via barycentric interpolation inside
//! the drawable's triangles. Answers "which way is up" and "how far above the
//! canvas top does the halo artwork reach", for the preset standard model.
#![allow(clippy::print_stdout)]

use bongocat_live2d::Live2dModel;
use bongocat_live2d_playback::{ExpressionClip, ExpressionLayer};
use bongocat_model::{ModelId, ModelPackageLimits, PresetModelCatalog};
use bongocat_render::RenderSnapshot;
use std::path::Path;
use std::time::Duration;

type Target = (&'static str, usize, f32, f32, &'static [usize]);

const HALO_TARGETS: &[Target] = &[
    // texture_02 ring ellipse pixels -> mesh uv.v = 1 - pixel_y/512
    ("ring-top", 2, 0.112, 0.219, &[18, 19]),
    ("ring-bottom", 2, 0.112, 0.082, &[18, 19]),
    ("ring-left", 2, 0.041, 0.150, &[18, 19]),
    ("ring-right", 2, 0.184, 0.150, &[18, 19]),
    // glow blob pixels (170,80) (170,140) (170,210)
    ("glow-top", 2, 0.166, 0.844, &[18, 19]),
    ("glow-center", 2, 0.166, 0.727, &[18, 19]),
    ("glow-bottom", 2, 0.166, 0.590, &[18, 19]),
];

const THUG_TARGETS: &[Target] = &[
    ("lens-center", 2, 0.684, 0.609, &[16, 17]),
    ("temple-end", 2, 0.645, 0.160, &[16, 17]),
    ("cigar", 2, 0.034, 0.404, &[16, 17]),
];

// body sanity anchors from texture_00: left eye dot, mouth
const BODY_TARGETS: &[Target] = &[
    ("left-eye", 0, 0.410, 0.918, &[0, 1, 3, 5, 6, 7, 14]),
    ("mouth", 0, 0.444, 0.697, &[0, 1, 3, 5, 6, 7, 14]),
];

fn locate(snapshot: &RenderSnapshot, target: &Target) -> Option<(usize, [f32; 2])> {
    let (_, texture, u, v, ids) = *target;
    for d in &snapshot.drawables {
        if d.texture_id.index() != texture || !ids.contains(&(d.id.index())) {
            continue;
        }
        for triangle in d.indices.chunks_exact(3) {
            let [a, b, c] = [
                d.vertices[triangle[0] as usize],
                d.vertices[triangle[1] as usize],
                d.vertices[triangle[2] as usize],
            ];
            let (u0, v0) = (a.uv[0], a.uv[1]);
            let (u1, v1) = (b.uv[0], b.uv[1]);
            let (u2, v2) = (c.uv[0], c.uv[1]);
            let det = (v1 - v2) * (u0 - u2) + (u2 - u1) * (v0 - v2);
            if det.abs() < 1e-12 {
                continue;
            }
            let l0 = ((v1 - v2) * (u - u2) + (u2 - u1) * (v - v2)) / det;
            let l1 = ((v2 - v0) * (u - u2) + (u0 - u2) * (v - v2)) / det;
            let l2 = 1.0 - l0 - l1;
            if l0 >= -1e-4 && l1 >= -1e-4 && l2 >= -1e-4 {
                let x = l0 * a.position[0] + l1 * b.position[0] + l2 * c.position[0];
                let y = l0 * a.position[1] + l1 * b.position[1] + l2 * c.position[1];
                return Some((d.id.index(), [x, y]));
            }
        }
    }
    None
}

fn report(label: &str, snapshot: &RenderSnapshot, targets: &[Target]) {
    println!("== {label} ==");
    for target in targets {
        match locate(snapshot, target) {
            Some((id, [x, y])) => {
                let from_top = (0.5 - y) * 100.0;
                println!(
                    "  {:<12} -> drawable {id} model ({x:.3}, {y:.3})  y-up: {from_top:.0}% from canvas top",
                    target.0
                );
            }
            None => println!("  {:<12} -> not found", target.0),
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/models");
    let catalog = PresetModelCatalog::open(root, ModelPackageLimits::default()).expect("catalog");
    let committed = catalog
        .load(&ModelId::parse("standard").expect("model id"))
        .expect("preset model");
    let mut model = Live2dModel::load(&committed).expect("live2d model");

    model.restore_parameter_defaults().expect("defaults");
    let base = model.update_and_snapshot().expect("snapshot");
    println!("canvas: {:?}", base.canvas);
    println!("bounds: {:?}", base.bounds);
    report(
        "base / body anchors (sanity: eye ~50-56% from top)",
        &base,
        BODY_TARGETS,
    );
    report("base / halo props", &base, HALO_TARGETS);
    report("base / thug props", &base, THUG_TARGETS);

    let bytes = std::fs::read(committed.root().join("live2d_expression2.exp3.json")).expect("expr");
    let halo_clip = ExpressionClip::from_slice(&bytes).expect("halo clip");
    model
        .apply_expression_layers(&[ExpressionLayer {
            clip: &halo_clip,
            weight: 1.0,
        }])
        .expect("apply halo");
    model
        .apply_automatic_effects(Duration::ZERO, 1.0)
        .expect("auto");
    let halo = model.update_and_snapshot().expect("snapshot");
    report("ascend (Param5=1)", &halo, HALO_TARGETS);

    model.restore_parameter_defaults().expect("defaults");
    model
        .set_normalized_parameter_by_id("Param4", 1.0)
        .expect("param4");
    let thug = model.update_and_snapshot().expect("snapshot");
    report("thug life (Param4=1)", &thug, THUG_TARGETS);

    // Topmost vertices of the halo meshes and the lightning overlay quads, with
    // their UVs, so the texture pixel at each can be checked for real artwork.
    for (label, snapshot, ids) in [
        ("ascend", &halo, &[18usize, 19][..]),
        ("thug", &thug, &[16usize, 17][..]),
    ] {
        for d in &snapshot.drawables {
            if !ids.contains(&(d.id.index())) {
                continue;
            }
            println!(
                "== masks: {label} drawable {} masks={:?} inverted={} double_sided={} render={} ==",
                d.id.index(),
                d.masks.iter().map(|m| m.index()).collect::<Vec<_>>(),
                d.inverted_mask,
                d.double_sided,
                d.render_order,
            );
            let mut top: Vec<([f32; 2], [f32; 2])> =
                d.vertices.iter().map(|v| (v.uv, v.position)).collect();
            top.sort_by(|a, b| b.1[1].partial_cmp(&a.1[1]).expect("finite"));
            println!("== top vertices: {label} drawable {} ==", d.id.index());
            for (uv, position) in top.iter().take(3) {
                println!(
                    "  model ({:.3}, {:.3})  uv ({:.4}, {:.4})",
                    position[0], position[1], uv[0], uv[1]
                );
            }
        }
    }
}
