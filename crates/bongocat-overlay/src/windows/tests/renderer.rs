//! Fitting a model into the window without distorting it.

use super::*;

#[test]
fn model_transform_preserves_aspect_ratio() {
    let canvas = CanvasInfo {
        width: 2048.0,
        height: 2048.0,
        origin_x: 1024.0,
        origin_y: 1024.0,
        pixels_per_unit: 1024.0,
    };
    assert_eq!(
        model_transform(ModelBounds::from_canvas(canvas), 800.0, 800.0, false),
        [1.0, 1.0, -0.0, -0.0]
    );
    assert_eq!(
        model_transform(ModelBounds::from_canvas(canvas), 1600.0, 800.0, false),
        [0.5, 1.0, -0.0, -0.0]
    );
    assert_eq!(
        model_transform(ModelBounds::from_canvas(canvas), 800.0, 800.0, true),
        [-1.0, 1.0, 0.0, -0.0]
    );
}

#[test]
#[ignore = "requires Windows D3D11 and DirectComposition"]
fn chat_bubble_is_visible_in_offscreen_d3d11_capture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repository root");
    let model =
        PresetModelCatalog::open(root.join("resources/models"), ModelPackageLimits::default())
            .expect("preset catalog")
            .load(&ModelId::parse("standard").expect("model id"))
            .expect("preset model");
    let mut session = CoverCaptureSession::start(Arc::new(model)).expect("offscreen renderer");
    session
        .runtime
        .client()
        .send(RuntimeCommand::ShowChatBubble {
            sender: "测试用户".to_owned(),
            content: "你好，聊天气泡应该完整显示。".to_owned(),
        })
        .expect("show bubble");
    let deadline = Instant::now() + Duration::from_secs(5);
    let frame = loop {
        assert!(
            Instant::now() < deadline,
            "runtime must publish the chat bubble"
        );
        pump_window_messages();
        if let Some(frame) = session.render_consumer.take_latest()
            && frame
                .snapshot
                .chat_bubble
                .as_ref()
                .is_some_and(|bubble| bubble.opacity >= 0.99)
        {
            break frame;
        }
        thread::sleep(session.frame_interval());
    };
    let bubble = frame.snapshot.chat_bubble.as_ref().expect("bubble");
    let renderer = &mut session.overlay.renderer;
    let [sx, sy, ox, oy] = model_transform(
        frame.snapshot.bounds,
        renderer.width as f32,
        renderer.height as f32,
        false,
    );
    let top = (bubble.anchor[1] + bubble.size[1]) * sy + oy;
    let bottom = bubble.anchor[1] * sy + oy;
    assert!(
        top < 1.0 && bottom > -1.0,
        "entire bubble is within clip space"
    );
    assert!((bubble.anchor[0] - bubble.size[0] / 2.0) * sx + ox > -1.0);
    assert!((bubble.anchor[0] + bubble.size[0] / 2.0) * sx + ox < 1.0);

    let mut baseline = frame.clone();
    let mut baseline_snapshot = (*frame.snapshot).clone();
    baseline_snapshot.chat_bubble = None;
    baseline.snapshot = Arc::new(baseline_snapshot);
    renderer.sync_frame(&baseline).expect("baseline frame");
    let before = renderer.draw_capturing(false).expect("baseline capture");
    renderer.sync_frame(&frame).expect("bubble frame");
    let after = renderer.draw_capturing(false).expect("bubble capture");
    let changed_rows = before
        .pixels()
        .chunks_exact(renderer.width as usize * 4)
        .zip(after.pixels().chunks_exact(renderer.width as usize * 4))
        .filter(|(before, after)| before != after)
        .count();
    let expected_rows = (top - bottom) * renderer.height as f32 / 2.0;
    assert!(
        changed_rows as f32 > expected_rows * 0.7,
        "bubble body must render, not just the bottom edge: {changed_rows}/{expected_rows}"
    );
    if let Some(path) = std::env::var_os("BONGOCAT_BUBBLE_CAPTURE_PATH") {
        image::save_buffer(
            path,
            after.pixels(),
            renderer.width,
            renderer.height,
            image::ColorType::Rgba8,
        )
        .expect("capture PNG");
    }
    session.finish().expect("capture runtime shutdown");
}
