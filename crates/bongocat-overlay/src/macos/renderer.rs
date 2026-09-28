//! Drawing one frame.
//!
//! The renderer owns the `CAMetalLayer`, the render pass and the panel it
//! presents to, and it composites the model over a transparent drawable in one
//! pass. It reads only the snapshot it is handed: deciding what to show is the
//! runtime's job, not the renderer's, so a frame can never change the product's
//! state.

use super::*;

pub(crate) const FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

pub(crate) const RUNTIME_TIMEOUT: Duration = Duration::from_millis(250);

pub(crate) const METAL_COMPLETION_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) struct NativeOverlay {
    pub(crate) panel: ManuallyDrop<Retained<NSPanel>>,
    pub(crate) device: Device,
    pub(crate) layer: MetalLayer,
    pub(crate) queue: CommandQueue,
    pub(crate) pipelines: Pipelines,
    pub(crate) sampler: SamplerState,
    pub(crate) model_generation: u64,
    pub(crate) resources: Arc<RenderResources>,
    pub(crate) model: GpuModel,
    pub(crate) presentation: OverlayPresentationState,
    pub(crate) corner_radius_percent: u8,
    /// Window alpha currently applied to the panel, including the hover fade.
    /// It lives here rather than on the session so replacing the native window
    /// resets it together with the panel that carries it.
    pub(crate) applied_alpha: f64,
    pub(crate) applied_click_through: bool,
}

pub(crate) fn pump_application_events(application: &NSApplication) {
    autoreleasepool(|_| {
        let deadline = NSDate::dateWithTimeIntervalSinceNow(0.0);
        // SAFETY: AppKit exports this immutable process-global run-loop mode
        // for use on the application main thread.
        let run_loop_mode = unsafe { NSDefaultRunLoopMode };
        if let Some(event) = application.nextEventMatchingMask_untilDate_inMode_dequeue(
            NSEventMask::Any,
            Some(&deadline),
            run_loop_mode,
            true,
        ) {
            application.sendEvent(&event);
        }
        application.updateWindows();
    });
}

impl NativeOverlay {
    pub(crate) fn create(
        mtm: MainThreadMarker,
        frame: &RenderFrame,
        options: OverlaySessionOptions,
        bounds: Option<OverlayWindowBounds>,
    ) -> Result<Self, OverlayError> {
        // A saved box that no longer touches any display is only restorable when
        // the placement constraint can pull it back onto one. With the
        // constraint enabled the correction below is what makes such a box
        // usable, so the box is kept as the candidate and clamped; without it
        // there is nothing to recover the window with, and restoring a box that
        // no longer intersects a display would leave an unreachable window, so
        // the window falls back to the cursor's display instead.
        let bounds = bounds
            .filter(|bounds| options.keep_inside_screen || overlay_bounds_visible(mtm, *bounds));
        let (default_width, default_height) =
            model_window_dimensions(frame.snapshot.canvas, options.scale_percent);
        let window_width =
            bounds.map_or(f64::from(default_width), |bounds| f64::from(bounds.width));
        let window_height =
            bounds.map_or(f64::from(default_height), |bounds| f64::from(bounds.height));
        let origin = bounds.map_or_else(
            || centered_origin(mtm, window_width, window_height),
            |bounds| NSPoint::new(f64::from(bounds.x), f64::from(bounds.y)),
        );
        let candidate_bounds = OverlayWindowBounds::new(
            rounded_i32(origin.x)?,
            rounded_i32(origin.y)?,
            rounded_u32(window_width)?,
            rounded_u32(window_height)?,
        );
        // A brand new window has no drag to interrupt, so an unusable placement
        // is corrected immediately: this is the restore path for a saved box and
        // the centering path for a window that has never been placed.
        let origin = if options.keep_inside_screen {
            let screens = screen_bounds_all(mtm)?;
            if bounds_inside_screens(&screens, candidate_bounds) {
                origin
            } else {
                correction_for_screens(&screens, candidate_bounds).map_or(origin, |corrected| {
                    NSPoint::new(f64::from(corrected.x), f64::from(corrected.y))
                })
            }
        } else {
            origin
        };
        let window_frame = NSRect::new(origin, NSSize::new(window_width, window_height));
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel;
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            window_frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setOpaque(false);
        panel.setHasShadow(false);
        panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        // Apply presentation opacity once to the completed panel surface. The
        // renderer keeps model-authored alpha intact while all Live2D parts,
        // masks, background, and key overlays are blended.
        panel.setAlphaValue(f64::from(options.opacity_percent) / 100.0);
        panel.setLevel(main_window_level(options.always_on_top));
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        panel.setMovableByWindowBackground(true);
        panel.setIgnoresMouseEvents(options.click_through);

        let view = NSView::new(mtm);
        view.setWantsLayer(true);
        let device = Device::system_default()
            .ok_or_else(|| OverlayError::new("Metal device is unavailable"))?;
        let layer = MetalLayer::new();
        layer.set_device(&device);
        layer.set_pixel_format(COLOR_ATTACHMENT_FORMAT);
        layer.set_opaque(false);
        layer.set_presents_with_transaction(false);
        layer.set_framebuffer_only(false);
        let scale = panel.backingScaleFactor();
        layer.set_drawable_size(core_graphics_types::geometry::CGSize::new(
            window_width * scale,
            window_height * scale,
        ));
        // SAFETY: metal::MetalLayerRef and objc2 QuartzCore both wrap the
        // same Objective-C CAMetalLayer instance, which NSView retains.
        let layer_ref =
            unsafe { mem::transmute::<&metal::MetalLayerRef, &ObjcMetalLayer>(layer.as_ref()) };
        // A headless or temporarily occluded compositor must not leave
        // nextDrawable waiting forever; return None so the caller can report
        // a recoverable renderer failure instead.
        layer_ref.setAllowsNextDrawableTimeout(true);
        layer_ref.setMaximumDrawableCount(3);
        view.setLayer(Some(layer_ref));
        panel.setContentView(Some(&view));

        let pipelines = create_pipelines(&device)?;
        let sampler_descriptor = SamplerDescriptor::new();
        sampler_descriptor.set_min_filter(MTLSamplerMinMagFilter::Linear);
        sampler_descriptor.set_mag_filter(MTLSamplerMinMagFilter::Linear);
        sampler_descriptor.set_address_mode_s(MTLSamplerAddressMode::ClampToEdge);
        sampler_descriptor.set_address_mode_t(MTLSamplerAddressMode::ClampToEdge);
        let sampler = device.new_sampler(&sampler_descriptor);
        let drawable_width = layer.drawable_size().width.round() as u64;
        let drawable_height = layer.drawable_size().height.round() as u64;
        let model = GpuModel::prepare(
            &device,
            &frame.resources,
            &frame.snapshot,
            drawable_width,
            drawable_height,
        )?;
        Ok(Self {
            panel: ManuallyDrop::new(panel),
            device: device.clone(),
            layer,
            queue: device.new_command_queue(),
            pipelines,
            sampler,
            model_generation: frame.model_generation,
            resources: Arc::clone(&frame.resources),
            model,
            presentation: OverlayPresentationState::default(),
            corner_radius_percent: options.corner_radius_percent,
            applied_alpha: f64::from(options.opacity_percent) / 100.0,
            applied_click_through: options.click_through,
        })
    }

    /// Apply the per-frame presentation state without replacing the window.
    ///
    /// `alpha` is the configured window opacity multiplied by the hover fade,
    /// and `click_through` is the effective pointer routing. The alpha is
    /// applied once to the completed panel surface, not to each Live2D part.
    /// The hover hide forces pass-through on so an invisible overlay cannot
    /// swallow a click meant for whatever is underneath it.
    pub(crate) fn apply_presentation(&mut self, alpha: f64, click_through: bool) {
        if alpha != self.applied_alpha {
            self.panel.setAlphaValue(alpha);
            self.applied_alpha = alpha;
        }
        if click_through != self.applied_click_through {
            self.set_click_through(click_through);
            self.applied_click_through = click_through;
        }
    }

    /// Resize the existing panel and its Metal layer without replacing the
    /// renderer. This keeps scale changes on the same compositor surface, so a
    /// freshly-created panel cannot flash transparent before its first frame.
    pub(crate) fn resize(&mut self, bounds: OverlayWindowBounds) -> Result<(), OverlayError> {
        let bounds = bounds.validate()?;
        let current_frame = self.panel.frame();
        let current = OverlayWindowBounds::new(
            rounded_i32(current_frame.origin.x)?,
            rounded_i32(current_frame.origin.y)?,
            rounded_u32(current_frame.size.width)?,
            rounded_u32(current_frame.size.height)?,
        );
        if current == bounds {
            return Ok(());
        }
        let visible = self.panel.isVisible();
        self.panel.setFrame_display(
            NSRect::new(
                NSPoint::new(f64::from(bounds.x), f64::from(bounds.y)),
                NSSize::new(f64::from(bounds.width), f64::from(bounds.height)),
            ),
            true,
        );
        let resized = self.sync_window_size()?;
        if visible && resized {
            // A newly-sized CAMetalLayer drawable is empty until it is
            // presented. Fill it before returning to the frame loop so the
            // resized panel cannot expose a transparent compositor frame.
            match self.draw(false) {
                Ok(()) => {}
                // The normal frame path below owns retry/backoff for a
                // temporarily unavailable drawable.
                Err(error) if error.is_temporary_presentation_unavailable() => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Move the window to a corrected box without touching its size, z-order or
    /// activation. The caller has already compared the box against the live
    /// frame, so an unchanged origin is a no-op.
    pub(crate) fn set_origin(&self, bounds: OverlayWindowBounds) {
        let origin = NSPoint::new(f64::from(bounds.x), f64::from(bounds.y));
        if self.panel.frame().origin != origin {
            self.panel.setFrameOrigin(origin);
        }
    }

    /// Adapt the panel to a newly prepared model while keeping the live width.
    ///
    /// The model-switch probe updates the GPU model in place instead of
    /// replacing the panel, so it has to apply the same canvas-aspect rule as
    /// the product session explicitly. The resize happens after GPU
    /// preparation succeeds; a failed preparation therefore leaves the old
    /// window geometry untouched.
    pub(crate) fn resize_for_model(&mut self, canvas: CanvasInfo) -> Result<(), OverlayError> {
        let frame = self.panel.frame();
        let current = OverlayWindowBounds::new(
            rounded_i32(frame.origin.x)?,
            rounded_i32(frame.origin.y)?,
            rounded_u32(frame.size.width)?,
            rounded_u32(frame.size.height)?,
        );
        let target = model_switch_window_bounds(current, canvas);
        let target_frame = NSRect::new(
            NSPoint::new(f64::from(target.x), f64::from(target.y)),
            NSSize::new(f64::from(target.width), f64::from(target.height)),
        );
        if frame.origin.x != target_frame.origin.x
            || frame.origin.y != target_frame.origin.y
            || frame.size.width != target_frame.size.width
            || frame.size.height != target_frame.size.height
        {
            self.panel.setFrame_display(target_frame, true);
        }
        self.sync_window_size()?;
        Ok(())
    }

    pub(crate) fn sync_frame(&mut self, frame: &RenderFrame) -> Result<bool, OverlayError> {
        if frame.model_generation != self.model_generation {
            validate_model_generation_advance(self.model_generation, frame.model_generation)?;
            let drawable_width = self.layer.drawable_size().width.round() as u64;
            let drawable_height = self.layer.drawable_size().height.round() as u64;
            let prepared = GpuModel::prepare(
                &self.device,
                &frame.resources,
                &frame.snapshot,
                drawable_width,
                drawable_height,
            )?;
            self.model = prepared;
            self.resources = Arc::clone(&frame.resources);
            self.model_generation = frame.model_generation;
            return Ok(true);
        }
        if !Arc::ptr_eq(&self.resources, &frame.resources) {
            return Err(OverlayError::new(
                "render resources changed within one model generation",
            ));
        }
        self.model.sync_snapshot(&frame.snapshot)?;
        Ok(false)
    }

    pub(crate) fn draw(&mut self, verify_frame: bool) -> Result<(), OverlayError> {
        autoreleasepool(|_| self.draw_in_autorelease_pool(verify_frame, false))?;
        self.presentation.record_presented_frame();
        Ok(())
    }

    /// Draw one frame and read it back as cover pixels.
    ///
    /// The window is never ordered front for this: the frame is drawn into the
    /// layer's drawable and read from it before anything reaches the screen, so
    /// the capture cannot flash a window the user did not ask for.
    pub(crate) fn draw_capturing(
        &mut self,
        verify_frame: bool,
    ) -> Result<CapturedFrame, OverlayError> {
        let captured = autoreleasepool(|_| self.draw_in_autorelease_pool(verify_frame, true))?;
        self.presentation.record_presented_frame();
        captured.ok_or_else(|| OverlayError::new("captured frame was not read back"))
    }

    pub(crate) fn set_visible(&self, visible: bool) -> Result<(), OverlayError> {
        if visible {
            self.presentation.require_presented_frame()?;
            // The frame loop calls this method after every successful draw.
            // Ordering an already visible panel frontmost would continually
            // raise it above other windows, even when always-on-top is off.
            if !self.panel.isVisible() {
                self.panel.orderFrontRegardless();
            }
            if !self.panel.isVisible() {
                return Err(OverlayError::new("macOS overlay did not become visible"));
            }
        } else if self.panel.isVisible() {
            self.panel.orderOut(None);
        }
        Ok(())
    }

    pub(crate) fn set_always_on_top(&self, always_on_top: bool) {
        self.panel.setLevel(main_window_level(always_on_top));
    }

    pub(crate) fn set_click_through(&self, click_through: bool) {
        self.panel.setIgnoresMouseEvents(click_through);
    }

    /// Align the Metal drawable and the mask targets with the panel's frame.
    ///
    /// A right-button resize drag changes the panel frame directly, so the
    /// layer and the mask textures keep the size they were created with until
    /// this runs. Everything else in the renderer reads the drawable size per
    /// frame — the model transform, the corner radius, the mask uniforms — so
    /// re-sizing the drawable is what makes the next frame match the window.
    ///
    /// Returns whether the size changed.
    pub(crate) fn sync_window_size(&mut self) -> Result<bool, OverlayError> {
        let frame = self.panel.frame();
        let backing = self.panel.backingScaleFactor();
        let width = (frame.size.width * backing).round().max(1.0) as u64;
        let height = (frame.size.height * backing).round().max(1.0) as u64;
        let current = self.layer.drawable_size();
        if current.width.round().max(1.0) as u64 == width
            && current.height.round().max(1.0) as u64 == height
        {
            return Ok(false);
        }
        self.layer
            .set_drawable_size(core_graphics_types::geometry::CGSize::new(
                width as f64,
                height as f64,
            ));
        self.model.resize_masks(&self.device, width, height);
        Ok(true)
    }

    pub(crate) fn draw_in_autorelease_pool(
        &self,
        verify_frame: bool,
        capture_frame: bool,
    ) -> Result<Option<CapturedFrame>, OverlayError> {
        let drawable = self.layer.next_drawable().ok_or_else(|| {
            OverlayError::temporary_presentation_unavailable("CAMetalLayer returned no drawable")
        })?;
        let pass = RenderPassDescriptor::new();
        let attachment = pass
            .color_attachments()
            .object_at(0)
            .ok_or_else(|| OverlayError::new("Metal color attachment is unavailable"))?;
        attachment.set_texture(Some(drawable.texture()));
        attachment.set_load_action(MTLLoadAction::Clear);
        attachment.set_store_action(MTLStoreAction::Store);
        attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
        let command_buffer = self.queue.new_command_buffer();
        let scale_offset = model_transform(
            self.model.bounds,
            drawable.texture().width() as f32,
            drawable.texture().height() as f32,
            self.model.mirror_horizontal,
        );
        let corner_radius = corner_radius_uniform(
            self.corner_radius_percent,
            drawable.texture().width() as f32,
            drawable.texture().height() as f32,
        );
        for mesh in &self.model.meshes {
            let Some(mask_texture) = &mesh.mask_texture else {
                continue;
            };
            let mask_pass = RenderPassDescriptor::new();
            let mask_attachment = mask_pass
                .color_attachments()
                .object_at(0)
                .ok_or_else(|| OverlayError::new("Metal mask attachment is unavailable"))?;
            mask_attachment.set_texture(Some(mask_texture));
            mask_attachment.set_load_action(MTLLoadAction::Clear);
            mask_attachment.set_store_action(MTLStoreAction::Store);
            mask_attachment.set_clear_color(MTLClearColor::new(0.0, 0.0, 0.0, 0.0));
            let mask_encoder = command_buffer.new_render_command_encoder(mask_pass);
            mask_encoder.set_front_facing_winding(MTLWinding::CounterClockwise);
            mask_encoder.set_render_pipeline_state(&self.pipelines.mask);
            for source_id in &mesh.masks {
                let source = self
                    .model
                    .meshes
                    .iter()
                    .find(|source| source.id == *source_id)
                    .ok_or_else(|| {
                        OverlayError::new(format!("mask source {source_id} is unavailable"))
                    })?;
                let uniforms = Uniforms {
                    scale_offset,
                    multiply_color: [1.0; 4],
                    screen_color: [0.0; 4],
                    mask_settings: [0.0; 4],
                    corner_radius: [0.0; 4],
                    opacity: 1.0,
                    padding: [0.0; 3],
                };
                mask_encoder.set_vertex_buffer(0, Some(&source.vertex_buffer), 0);
                mask_encoder.set_vertex_bytes(
                    1,
                    size_of::<Uniforms>() as u64,
                    std::ptr::from_ref(&uniforms).cast(),
                );
                let texture = self.model.textures.get(&source.texture_id).ok_or_else(|| {
                    OverlayError::new(format!("texture {} is unavailable", source.texture_id))
                })?;
                mask_encoder.set_cull_mode(metal_cull_mode(
                    source.double_sided,
                    self.model.mirror_horizontal,
                ));
                mask_encoder.set_fragment_texture(0, Some(texture));
                mask_encoder.set_fragment_sampler_state(0, Some(&self.sampler));
                mask_encoder.draw_indexed_primitives(
                    MTLPrimitiveType::Triangle,
                    source.index_count,
                    MTLIndexType::UInt16,
                    &source.index_buffer,
                    0,
                );
            }
            mask_encoder.end_encoding();
        }

        let encoder = command_buffer.new_render_command_encoder(pass);
        encoder.set_front_facing_winding(MTLWinding::CounterClockwise);
        if let Some(background) = &self.model.background {
            encoder.set_cull_mode(MTLCullMode::None);
            let uniforms = Uniforms {
                scale_offset,
                multiply_color: [1.0; 4],
                screen_color: [0.0; 4],
                mask_settings: [0.0; 4],
                corner_radius,
                opacity: 1.0,
                padding: [0.0; 3],
            };
            encoder.set_render_pipeline_state(&self.pipelines.normal);
            encoder.set_vertex_buffer(0, Some(&self.model.background_vertex_buffer), 0);
            encoder.set_vertex_bytes(
                1,
                size_of::<Uniforms>() as u64,
                std::ptr::from_ref(&uniforms).cast(),
            );
            encoder.set_fragment_bytes(
                1,
                size_of::<Uniforms>() as u64,
                std::ptr::from_ref(&uniforms).cast(),
            );
            encoder.set_fragment_texture(0, Some(background));
            encoder.set_fragment_texture(1, Some(&self.model.empty_mask));
            encoder.set_fragment_sampler_state(0, Some(&self.sampler));
            encoder.draw_indexed_primitives(
                MTLPrimitiveType::Triangle,
                6,
                MTLIndexType::UInt16,
                &self.model.background_index_buffer,
                0,
            );
        }
        for mesh in &self.model.meshes {
            if !mesh.visible || mesh.opacity <= 0.0 {
                continue;
            }
            let mask_texture = &mesh.mask_texture;
            encoder.set_cull_mode(metal_cull_mode(
                mesh.double_sided,
                self.model.mirror_horizontal,
            ));
            let uniforms = Uniforms {
                scale_offset,
                multiply_color: mesh.multiply_color,
                screen_color: mesh.screen_color,
                mask_settings: [
                    drawable.texture().width() as f32,
                    drawable.texture().height() as f32,
                    f32::from(mask_texture.is_some()),
                    f32::from(mesh.inverted_mask),
                ],
                corner_radius,
                opacity: mesh.opacity * self.model.model_opacity,
                padding: [0.0; 3],
            };
            encoder.set_render_pipeline_state(self.pipelines.for_mode(mesh.blend_mode));
            encoder.set_vertex_buffer(0, Some(&mesh.vertex_buffer), 0);
            encoder.set_vertex_bytes(
                1,
                size_of::<Uniforms>() as u64,
                std::ptr::from_ref(&uniforms).cast(),
            );
            encoder.set_fragment_bytes(
                1,
                size_of::<Uniforms>() as u64,
                std::ptr::from_ref(&uniforms).cast(),
            );
            let texture = self.model.textures.get(&mesh.texture_id).ok_or_else(|| {
                OverlayError::new(format!("texture {} is unavailable", mesh.texture_id))
            })?;
            encoder.set_fragment_texture(0, Some(texture));
            encoder.set_fragment_texture(
                1,
                Some(mask_texture.as_ref().unwrap_or(&self.model.empty_mask)),
            );
            encoder.set_fragment_sampler_state(0, Some(&self.sampler));
            encoder.draw_indexed_primitives(
                MTLPrimitiveType::Triangle,
                mesh.index_count,
                MTLIndexType::UInt16,
                &mesh.index_buffer,
                0,
            );
        }
        // Key overlays are the topmost layer so pressed-key imagery remains
        // visible above both the background and Live2D model drawables.
        for overlay in &self.model.active_keys {
            let Some(texture) = self.model.key_textures.get(&overlay.asset_id) else {
                continue;
            };
            encoder.set_cull_mode(MTLCullMode::None);
            let uniforms = Uniforms {
                scale_offset,
                multiply_color: [1.0; 4],
                screen_color: [0.0; 4],
                mask_settings: [0.0; 4],
                corner_radius,
                opacity: 1.0,
                padding: [0.0; 3],
            };
            encoder.set_render_pipeline_state(&self.pipelines.normal);
            encoder.set_vertex_buffer(0, Some(&self.model.background_vertex_buffer), 0);
            encoder.set_vertex_bytes(
                1,
                size_of::<Uniforms>() as u64,
                std::ptr::from_ref(&uniforms).cast(),
            );
            encoder.set_fragment_bytes(
                1,
                size_of::<Uniforms>() as u64,
                std::ptr::from_ref(&uniforms).cast(),
            );
            encoder.set_fragment_texture(0, Some(texture));
            encoder.set_fragment_texture(1, Some(&self.model.empty_mask));
            encoder.set_fragment_sampler_state(0, Some(&self.sampler));
            encoder.draw_indexed_primitives(
                MTLPrimitiveType::Triangle,
                6,
                MTLIndexType::UInt16,
                &self.model.background_index_buffer,
                0,
            );
        }
        encoder.end_encoding();
        // Shared per-drawable buffers cannot be rewritten until this frame
        // retires. A later renderer revision will replace this correctness
        // fence with multiple in-flight frame resources.
        //
        // The fence parks this thread in the kernel on a Metal completion
        // handler rather than polling `status()`. Polling on a sleep loop woke
        // the frame thread about a thousand times a second for a wait that
        // usually lasts a few milliseconds, and it held the AppKit event pump
        // out of the loop for the whole wait. The handler is registered before
        // `commit()` so a buffer that retires immediately still signals, and
        // the `Arc` keeps the flag alive if the deadline is ever reached first
        // and the handler fires later.
        let retired = Arc::new((Mutex::new(false), Condvar::new()));
        let signal = Arc::clone(&retired);
        // The handler is declared over the object pointer Metal actually passes
        // (`id<MTLCommandBuffer>`) rather than the `metal` crate's reference
        // wrapper, which `objc2` cannot encode. The block ABI is the same: one
        // pointer argument, no return.
        let handler: RcBlock<dyn Fn(NonNull<AnyObject>)> =
            RcBlock::new(move |_command_buffer: NonNull<AnyObject>| {
                let (retired, ready) = &*signal;
                *retired
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
                ready.notify_all();
            });
        // SAFETY: `addCompletedHandler:` takes a `void (^)(id<MTLCommandBuffer>)`
        // block, which is exactly the block `handler` is. The `metal` crate's
        // wrapper is typed against the deprecated `block` crate rather than
        // `block2`, so the message is sent through `objc2` over the same
        // object pointer with the same block-pointer ABI. Metal retains the
        // block, and the block owns the `Arc` it signals through, so the flag
        // outlives this frame even when the wait below gives up first.
        let command_buffer_object: NonNull<AnyObject> =
            NonNull::new(command_buffer.as_ptr() as *mut AnyObject)
                .ok_or_else(|| OverlayError::new("Metal returned a null command buffer"))?;
        unsafe {
            let () = msg_send![command_buffer_object, addCompletedHandler: &*handler];
        }
        command_buffer.present_drawable(drawable);
        command_buffer.commit();
        {
            let (retired, ready) = &*retired;
            let mut done = retired
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            while !*done {
                let (guard, timeout) = ready
                    .wait_timeout(done, METAL_COMPLETION_TIMEOUT)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                done = guard;
                if timeout.timed_out() {
                    break;
                }
            }
        }
        if command_buffer.status() != MTLCommandBufferStatus::Completed {
            return Err(OverlayError::new(format!(
                "Metal command buffer ended with {:?}",
                command_buffer.status()
            )));
        }
        if verify_frame {
            verify_frame_smoke(drawable.texture())?;
        }
        if capture_frame {
            return read_drawable_frame(drawable.texture()).map(Some);
        }
        Ok(None)
    }

    pub(crate) fn current_allocated_size(&self) -> u64 {
        self.device.current_allocated_size()
    }
}

impl Drop for NativeOverlay {
    fn drop(&mut self) {
        self.panel.setContentView(None);
        // SAFETY: releasedWhenClosed transfers the panel retain to AppKit's
        // close path. The ManuallyDrop field is not touched afterwards.
        unsafe { self.panel.setReleasedWhenClosed(true) };
        self.panel.close();
    }
}

pub(crate) fn model_transform(
    bounds: ModelBounds,
    width: f32,
    height: f32,
    mirror_horizontal: bool,
) -> [f32; 4] {
    let model_width = bounds.width();
    let model_height = bounds.height();
    let center = bounds.center();
    let center_x = center[0];
    let center_y = center[1];
    let model_aspect = model_width / model_height;
    let viewport_aspect = width / height;
    let (mut scale_x, mut scale_y) = (2.0 / model_width, 2.0 / model_height);
    if viewport_aspect > model_aspect {
        scale_x *= model_aspect / viewport_aspect;
    } else {
        scale_y *= viewport_aspect / model_aspect;
    }
    let mut offset_x = -center_x * scale_x;
    if mirror_horizontal {
        scale_x = -scale_x;
        offset_x = -offset_x;
    }
    [scale_x, scale_y, offset_x, -center_y * scale_y]
}
