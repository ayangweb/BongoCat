use super::{InputSource, InputSourceError};
use objc2::{
    AnyThread, MainThreadMarker,
    encode::{Encode, Encoding, RefEncode},
    msg_send,
    rc::{Retained, autoreleasepool},
    runtime::{AnyClass, AnyObject},
};
use objc2_app_kit::NSImage;
use objc2_core_foundation::{
    CFDictionary, CFNotificationCenter, CFNotificationName, CFNotificationSuspensionBehavior,
    CFRetained, CFString, CFType, CFURL, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo,
};
use objc2_foundation::{NSURL, ns_string};
use std::{
    ffi::c_void,
    ptr::{self, NonNull},
    sync::atomic::{AtomicBool, Ordering},
};

// objc2-carbon 0.3.2 is empty. These public Carbon TIS symbols are the only
// handwritten bindings; Copy ownership is discharged by CFRetained.
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C-unwind" {
    fn TISCopyCurrentKeyboardInputSource() -> Option<NonNull<CFType>>;
    fn TISGetInputSourceProperty(source: &CFType, key: &CFString) -> *const CFType;
    static kTISPropertyInputSourceID: &'static CFString;
    static kTISPropertyInputModeID: &'static CFString;
    static kTISPropertyLocalizedName: &'static CFString;
    static kTISPropertyIconImageURL: &'static CFString;
    static kTISNotifySelectedKeyboardInputSourceChanged: &'static CFString;
}

#[repr(C)]
struct ProcessSerialNumber {
    high: u32,
    low: u32,
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C-unwind" {
    fn TransformProcessType(process: &ProcessSerialNumber, state: u32) -> i32;
}

fn become_agent() -> Result<(), InputSourceError> {
    const CURRENT_PROCESS: u32 = 2;
    const UI_ELEMENT_APPLICATION: u32 = 4;
    let current_process = ProcessSerialNumber {
        high: 0,
        low: CURRENT_PROCESS,
    };
    // SAFETY: kCurrentProcess identifies this process; state 4 is Apple's
    // kProcessTransformToUIElementApplication. Like Tao's Dock implementation,
    // classify the process before TIS/AppKit can register it as a foreground app.
    // The plugin has no windows or NSApplication event loop to finish launching.
    let status = unsafe { TransformProcessType(&current_process, UI_ELEMENT_APPLICATION) };
    (status == 0).then_some(()).ok_or(InputSourceError)
}

/// The input source the system's own icon manager takes. Its parameter is
/// declared as `struct __TISInputSource *`, and a checked message send compares
/// encodings, so that pointer shape is spelled out here.
#[repr(C)]
struct TisInputSource {
    _private: [u8; 0],
}

// SAFETY: the type is only ever built from a pointer to a live TIS input
// source, and `@encode(struct __TISInputSource *)` is what the runtime reports
// for `-[KLInputSourceIconManager imageForTISInputSource:]`.
unsafe impl Encode for TisInputSource {
    const ENCODING: Encoding = Encoding::Struct("__TISInputSource", &[]);
}

// SAFETY: as above; a reference to the type is that struct pointer.
unsafe impl RefEncode for TisInputSource {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}

pub(super) struct Monitor {
    // MainThreadMarker also makes this owner !Send/!Sync.
    _main: MainThreadMarker,
    center: CFRetained<CFNotificationCenter>,
    dirty: Box<AtomicBool>,
    cached: Option<InputSource>,
}

impl Monitor {
    pub(super) fn new() -> Result<Self, InputSourceError> {
        let main = MainThreadMarker::new().ok_or(InputSourceError)?;
        become_agent()?;
        let center = CFNotificationCenter::distributed_center().ok_or(InputSourceError)?;
        let dirty = Box::new(AtomicBool::new(true));
        // SAFETY: the stable Box address outlives the registration. The callback
        // only sets an atomic flag; Drop unregisters it on this same main thread.
        unsafe {
            center.add_observer(
                (&*dirty as *const AtomicBool).cast(),
                Some(changed),
                Some(kTISNotifySelectedKeyboardInputSourceChanged),
                ptr::null(),
                CFNotificationSuspensionBehavior::DeliverImmediately,
            );
        }
        Ok(Self {
            _main: main,
            center,
            dirty,
            cached: None,
        })
    }

    pub(super) fn changed(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    pub(super) fn snapshot(&mut self) -> Result<InputSource, InputSourceError> {
        self.dirty.store(false, Ordering::Relaxed);
        let result = autoreleasepool(|_| read_source(self.cached.as_ref()));
        self.cached = result.as_ref().ok().cloned();
        result
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        // SAFETY: same center/name/address as registration, still on the main
        // thread; dirty is alive until after this destructor completes.
        unsafe {
            self.center.remove_observer(
                (&*self.dirty as *const AtomicBool).cast(),
                Some(kTISNotifySelectedKeyboardInputSourceChanged),
                ptr::null(),
            );
        }
    }
}

unsafe extern "C-unwind" fn changed(
    _: *mut CFNotificationCenter,
    observer: *mut c_void,
    _: *const CFNotificationName,
    _: *const c_void,
    _: *const CFDictionary,
) {
    // SAFETY: registration supplies a live Box<AtomicBool> and unregisters it
    // before freeing. No native call, allocation or panic crosses this callback.
    if let Some(dirty) = unsafe { observer.cast::<AtomicBool>().as_ref() } {
        dirty.store(true, Ordering::Relaxed);
    }
}

fn property<'a>(source: &'a CFType, key: &CFString) -> Option<&'a CFType> {
    // SAFETY: source is a live Copy-owned TIS object. Its borrowed property is
    // valid until source drops and is runtime-type-checked before use.
    unsafe { TISGetInputSourceProperty(source, key).as_ref() }
}

fn read_source(cached: Option<&InputSource>) -> Result<InputSource, InputSourceError> {
    // SAFETY: Copy transfers one ownership retain. All properties remain borrowed
    // within this scope. The exported constants are initialized by Carbon.
    unsafe {
        let source =
            CFRetained::from_raw(TISCopyCurrentKeyboardInputSource().ok_or(InputSourceError)?);
        let text = |key| {
            property(&source, key)?
                .downcast_ref::<CFString>()
                .map(ToString::to_string)
        };
        let id = text(kTISPropertyInputSourceID).ok_or(InputSourceError)?;
        let mode = text(kTISPropertyInputModeID).unwrap_or_default();
        let id = format!("{id}::{mode}");
        let name = text(kTISPropertyLocalizedName)
            .filter(|name| !name.is_empty())
            .ok_or(InputSourceError)?;
        if let Some(cached) = cached.filter(|cached| cached.id == id && cached.name == name) {
            return Ok(cached.clone());
        }
        // The system's own artwork comes first: it is what the input menu and
        // the switcher show, and for a keyboard layout it is the only icon that
        // exists. An input method that ships its icon instead is read from the
        // documented URL, and a source with neither renders as a badge.
        let icon_rgba = system_icon(&source)
            .as_deref()
            .and_then(rasterize_icon)
            .or_else(|| {
                property(&source, kTISPropertyIconImageURL)
                    .and_then(|value| value.downcast_ref::<CFURL>())
                    .and_then(load_icon_image)
                    .and_then(|image| rasterize_icon(&image))
            });
        Ok(InputSource {
            id,
            name,
            icon_rgba,
            // macOS names the source itself and draws its own artwork; there is no
            // separate system label to report.
            label: None,
        })
    }
}

/// The framework that registers the system's input source icon manager.
const SYSTEM_ICON_FRAMEWORK: &std::ffi::CStr =
    c"/System/Library/PrivateFrameworks/KeyboardLayouts.framework/KeyboardLayouts";

/// The icon the system itself draws for `source`, if it draws one.
///
/// Keyboard layouts carry no icon URL on current releases: the system renders
/// their icon from a label ("A" for ABC) with artwork that is only reachable
/// through its private icon manager, which is also what System Settings uses.
/// Anything missing here — framework, class, or image — leaves the URL
/// fallback and, after that, the badge in place.
fn system_icon(source: &CFType) -> Option<Retained<NSImage>> {
    // The manager caches its artwork for the life of the process, so the loaded
    // framework is deliberately never closed: dlclose could unload the class
    // while images derived from it are still in use. Later calls only raise the
    // reference count dlopen already holds.
    let handle = unsafe {
        libc::dlopen(
            SYSTEM_ICON_FRAMEWORK.as_ptr(),
            libc::RTLD_LAZY | libc::RTLD_LOCAL,
        )
    };
    if handle.is_null() {
        return None;
    }
    // SAFETY: the handle above keeps the framework's classes registered for the
    // rest of the process. `source` is a live TIS input source, both messages
    // exist on the class, and their results are autoreleased, so
    // `Retained::retain` takes ownership before the caller's pool drains.
    unsafe {
        let class = AnyClass::get(c"KLInputSourceIconManager")?;
        let manager: *mut AnyObject = msg_send![class, managerWithAllInputSources];
        let image: *mut NSImage = msg_send![
            manager,
            imageForTISInputSource: ptr::from_ref(source).cast::<TisInputSource>()
        ];
        Retained::retain(image)
    }
}

/// The image an input method ships, per the documented TIS icon URL.
fn load_icon_image(url: &CFURL) -> Option<Retained<NSImage>> {
    // SAFETY: CFURL and NSURL are toll-free bridged, and a URL that is not a
    // local file is rejected before AppKit touches the file system.
    unsafe {
        let url = &*(url as *const CFURL).cast::<NSURL>();
        if !url.isFileURL() {
            return None;
        }
        // Apple's TIS header explicitly permits a misdeclared extension. Try
        // the common local variants before falling back to a language badge.
        NSImage::initWithContentsOfURL(NSImage::alloc(), url).or_else(|| {
            let base = url.URLByDeletingPathExtension()?;
            [ns_string!("tiff"), ns_string!("png"), ns_string!("pdf")]
                .into_iter()
                .find_map(|extension| {
                    let alternate = base.URLByAppendingPathExtension(extension)?;
                    NSImage::initWithContentsOfURL(NSImage::alloc(), &alternate)
                })
        })
    }
}

fn rasterize_icon(image: &NSImage) -> Option<Vec<u8>> {
    // SAFETY: the NSImage and its CGImage remain alive throughout drawing; the
    // context borrows this fixed buffer and is dropped before its bytes are
    // accessed or moved.
    unsafe {
        let cg_image = image.CGImageForProposedRect_context_hints(ptr::null_mut(), None, None)?;
        let space = CGColorSpace::new_device_rgb()?;
        let mut pixels = vec![0; 32 * 32 * 4];
        let context = CGBitmapContextCreate(
            pixels.as_mut_ptr().cast(),
            32,
            32,
            8,
            128,
            Some(&space),
            CGImageByteOrderInfo::Order32Big.0 | CGImageAlphaInfo::PremultipliedLast.0,
        )?;
        CGContext::translate_ctm(Some(&context), 0.0, 32.0);
        CGContext::scale_ctm(Some(&context), 1.0, -1.0);
        CGContext::draw_image(
            Some(&context),
            fitted_rect(
                CGImage::width(Some(&cg_image)) as f64,
                CGImage::height(Some(&cg_image)) as f64,
            ),
            Some(&cg_image),
        );
        drop(context);
        for pixel in pixels.chunks_exact_mut(4) {
            let alpha = u32::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = (u32::from(*channel) * 255)
                    .checked_div(alpha)
                    .unwrap_or(0)
                    .min(255) as u8;
            }
        }
        // StatusIcon consumes row-major pixels from the top edge down. The
        // bitmap context exposes its rows in the opposite order, so normalize
        // the native image here rather than making the shared HUD guess which
        // platform produced an icon.
        flip_rows(&mut pixels);
        pixels
            .chunks_exact(4)
            .any(|pixel| pixel[3] != 0)
            .then_some(pixels)
    }
}

/// Where a native icon lands inside the fixed square slot the HUD reserves.
///
/// System artwork is not always square — an input source icon label is 44x32 —
/// so an icon is fitted into the slot and centered instead of being stretched
/// to fill it.
fn fitted_rect(width: f64, height: f64) -> CGRect {
    const SIDE: f64 = 32.0;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(SIDE, SIDE));
    }
    let scale = (SIDE / width).min(SIDE / height);
    let size = CGSize::new(width * scale, height * scale);
    CGRect::new(
        CGPoint::new((SIDE - size.width) / 2.0, (SIDE - size.height) / 2.0),
        size,
    )
}

fn flip_rows(pixels: &mut [u8]) {
    const ROW_BYTES: usize = 32 * 4;
    for row in 0..16 {
        let opposite = 31 - row;
        for byte in 0..ROW_BYTES {
            pixels.swap(row * ROW_BYTES + byte, opposite * ROW_BYTES + byte);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fitted_rect, flip_rows};

    #[test]
    fn native_icon_rows_are_normalized_to_top_down_order() {
        let mut pixels = vec![0; 32 * 32 * 4];
        pixels[..4].copy_from_slice(&[1, 2, 3, 4]);
        pixels[(31 * 32 * 4)..(32 * 32 * 4)].copy_from_slice(&[5, 6, 7, 8].repeat(32));

        flip_rows(&mut pixels);

        assert_eq!(&pixels[..4], &[5, 6, 7, 8]);
        assert_eq!(&pixels[(31 * 32 * 4)..(31 * 32 * 4 + 4)], &[1, 2, 3, 4]);
    }

    #[test]
    fn square_icons_fill_the_whole_slot() {
        let rect = fitted_rect(64.0, 64.0);

        assert_eq!(rect.origin.x, 0.0);
        assert_eq!(rect.origin.y, 0.0);
        assert_eq!(rect.size.width, 32.0);
        assert_eq!(rect.size.height, 32.0);
    }

    #[test]
    fn label_icons_keep_their_aspect_ratio_and_center() {
        // The system's input source label artwork is wider than it is tall, so
        // it is letterboxed rather than stretched into a square slot.
        let rect = fitted_rect(44.0, 32.0);

        assert_eq!(rect.size.width, 32.0);
        assert!((rect.size.height - (32.0 * 32.0 / 44.0)).abs() < 0.001);
        assert_eq!(rect.origin.x, 0.0);
        assert!((rect.origin.y - (32.0 - rect.size.height) / 2.0).abs() < 0.001);
    }

    #[test]
    fn unusable_icon_sizes_fall_back_to_the_full_slot() {
        for (width, height) in [(0.0, 0.0), (f64::NAN, 32.0), (32.0, -1.0)] {
            let rect = fitted_rect(width, height);

            assert_eq!(rect.size.width, 32.0);
            assert_eq!(rect.size.height, 32.0);
        }
    }
}
