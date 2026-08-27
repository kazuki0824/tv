use jni::objects::{JByteArray, JObject};
use jni::sys::{jboolean, jbyteArray, jint, jlong};
use jni::JNIEnv;
use std::collections::BTreeMap;
use std::os::raw::{c_int, c_longlong, c_void};
use std::ptr;
use std::slice;
use std::sync::{Mutex, OnceLock};

const ARIBCC_CAPTIONTYPE_CAPTION: c_int = 0x80;
const ARIBCC_CAPTIONTYPE_SUPERIMPOSE: c_int = 0x81;
const ARIBCC_ENCODING_SCHEME_ARIB_STD_B24_JIS: c_int = 1;
const ARIBCC_DECODE_STATUS_GOT_CAPTION: c_int = 2;
const ARIBCC_RENDER_STATUS_ERROR: c_int = 0;
const ARIBCC_RENDER_STATUS_NO_IMAGE: c_int = 1;
const ARIBCC_RENDER_STATUS_GOT_IMAGE: c_int = 2;
const ARIBCC_RENDER_STATUS_GOT_IMAGE_UNCHANGED: c_int = 3;
const ARIBCC_PROFILE_A: c_int = 0x0008;
const ARIBCC_PROFILE_C: c_int = 0x0012;
const ARIBCC_LANGUAGEID_FIRST: c_int = 1;
const ARIBCC_FONTPROVIDER_TYPE_AUTO: c_int = 0;
const ARIBCC_TEXTRENDERER_TYPE_AUTO: c_int = 0;
const ARIBCC_PIXELFORMAT_RGBA8888: c_int = 0;
const ARIBCC_PTS_NOPTS: c_longlong = i64::MIN;
const ARIBCC_DURATION_INDEFINITE: c_longlong = i64::MAX;
const MAX_FRAME_DIMENSION: c_int = 16_384;
const MAX_IMAGES_PER_FRAME: usize = 256;
const MAX_FRAME_BITMAP_BYTES: usize = 256 * 1024 * 1024;
const FRAME_HEADER_BYTES: usize = 8 + 8 + 4;
const IMAGE_HEADER_BYTES: usize = 6 * 4;

#[repr(C)]
struct AribccCaption {
    caption_type: c_int,
    flags: c_int,
    iso6392_language_code: u32,
    text: *mut c_void,
    regions: *mut c_void,
    region_count: u32,
    drcs_map: *mut c_void,
    pts: c_longlong,
    wait_duration: c_longlong,
    plane_width: c_int,
    plane_height: c_int,
    has_builtin_sound: bool,
    builtin_sound_id: u8,
}

impl Default for AribccCaption {
    fn default() -> Self {
        Self {
            caption_type: ARIBCC_CAPTIONTYPE_CAPTION,
            flags: 0,
            iso6392_language_code: 0,
            text: ptr::null_mut(),
            regions: ptr::null_mut(),
            region_count: 0,
            drcs_map: ptr::null_mut(),
            pts: ARIBCC_PTS_NOPTS,
            wait_duration: ARIBCC_DURATION_INDEFINITE,
            plane_width: 0,
            plane_height: 0,
            has_builtin_sound: false,
            builtin_sound_id: 0,
        }
    }
}

#[repr(C)]
struct AribccImage {
    width: c_int,
    height: c_int,
    stride: c_int,
    dst_x: c_int,
    dst_y: c_int,
    pixel_format: c_int,
    bitmap: *mut u8,
    bitmap_size: u32,
}

#[repr(C)]
struct AribccRenderResult {
    pts: c_longlong,
    duration: c_longlong,
    images: *mut AribccImage,
    image_count: u32,
}

impl Default for AribccRenderResult {
    fn default() -> Self {
        Self {
            pts: ARIBCC_PTS_NOPTS,
            duration: ARIBCC_DURATION_INDEFINITE,
            images: ptr::null_mut(),
            image_count: 0,
        }
    }
}

#[link(name = "aribcaption", kind = "static")]
extern "C" {
    fn aribcc_context_alloc() -> *mut c_void;
    fn aribcc_context_free(context: *mut c_void);
    fn aribcc_decoder_alloc(context: *mut c_void) -> *mut c_void;
    fn aribcc_decoder_free(decoder: *mut c_void);
    fn aribcc_decoder_initialize(
        decoder: *mut c_void,
        encoding_scheme: c_int,
        caption_type: c_int,
        profile: c_int,
        language_id: c_int,
    ) -> bool;
    fn aribcc_decoder_decode(
        decoder: *mut c_void,
        pes_data: *const u8,
        length: usize,
        pts: c_longlong,
        out_caption: *mut AribccCaption,
    ) -> c_int;
    fn aribcc_decoder_flush(decoder: *mut c_void);
    fn aribcc_caption_cleanup(caption: *mut AribccCaption);
    fn aribcc_renderer_alloc(context: *mut c_void) -> *mut c_void;
    fn aribcc_renderer_free(renderer: *mut c_void);
    fn aribcc_renderer_initialize(
        renderer: *mut c_void,
        caption_type: c_int,
        font_provider_type: c_int,
        text_renderer_type: c_int,
    ) -> bool;
    fn aribcc_renderer_set_frame_size(
        renderer: *mut c_void,
        frame_width: c_int,
        frame_height: c_int,
    ) -> bool;
    fn aribcc_renderer_append_caption(renderer: *mut c_void, caption: *const AribccCaption)
        -> bool;
    fn aribcc_renderer_render(
        renderer: *mut c_void,
        pts: c_longlong,
        out_result: *mut AribccRenderResult,
    ) -> c_int;
    fn aribcc_renderer_flush(renderer: *mut c_void);
    fn aribcc_render_result_cleanup(result: *mut AribccRenderResult);
}

struct RenderedCaptionImage {
    dst_x: i32,
    dst_y: i32,
    width: i32,
    height: i32,
    stride: i32,
    rgba8888: Vec<u8>,
}

struct RenderedCaptionFrame {
    pts_millis: i64,
    duration_millis: Option<i64>,
    images: Vec<RenderedCaptionImage>,
}

struct CaptionEngine {
    context: *mut c_void,
    decoder: *mut c_void,
    renderer: *mut c_void,
    viewport: Option<(i32, i32)>,
}

unsafe impl Send for CaptionEngine {}

impl Drop for CaptionEngine {
    fn drop(&mut self) {
        unsafe {
            if !self.renderer.is_null() {
                aribcc_renderer_free(self.renderer);
                self.renderer = ptr::null_mut();
            }
            if !self.decoder.is_null() {
                aribcc_decoder_free(self.decoder);
                self.decoder = ptr::null_mut();
            }
            if !self.context.is_null() {
                aribcc_context_free(self.context);
                self.context = ptr::null_mut();
            }
        }
    }
}

impl CaptionEngine {
    fn new(data_component_id: jint, superimpose: bool) -> Option<Self> {
        let caption_type = if superimpose {
            ARIBCC_CAPTIONTYPE_SUPERIMPOSE
        } else {
            ARIBCC_CAPTIONTYPE_CAPTION
        };
        let profile = if data_component_id == ARIBCC_PROFILE_C {
            ARIBCC_PROFILE_C
        } else {
            ARIBCC_PROFILE_A
        };
        unsafe {
            let context = aribcc_context_alloc();
            if context.is_null() {
                return None;
            }
            let decoder = aribcc_decoder_alloc(context);
            if decoder.is_null() {
                aribcc_context_free(context);
                return None;
            }
            if !aribcc_decoder_initialize(
                decoder,
                ARIBCC_ENCODING_SCHEME_ARIB_STD_B24_JIS,
                caption_type,
                profile,
                ARIBCC_LANGUAGEID_FIRST,
            ) {
                aribcc_decoder_free(decoder);
                aribcc_context_free(context);
                return None;
            }
            let renderer = aribcc_renderer_alloc(context);
            if renderer.is_null()
                || !aribcc_renderer_initialize(
                    renderer,
                    caption_type,
                    ARIBCC_FONTPROVIDER_TYPE_AUTO,
                    ARIBCC_TEXTRENDERER_TYPE_AUTO,
                )
            {
                if !renderer.is_null() {
                    aribcc_renderer_free(renderer);
                }
                aribcc_decoder_free(decoder);
                aribcc_context_free(context);
                return None;
            }
            Some(Self {
                context,
                decoder,
                renderer,
                viewport: None,
            })
        }
    }

    fn set_viewport(&mut self, width: jint, height: jint) -> bool {
        if width <= 0 || height <= 0 || width > MAX_FRAME_DIMENSION || height > MAX_FRAME_DIMENSION
        {
            return false;
        }
        let success = unsafe { aribcc_renderer_set_frame_size(self.renderer, width, height) };
        if success {
            self.viewport = Some((width, height));
        }
        success
    }

    fn flush(&mut self) {
        unsafe {
            aribcc_renderer_flush(self.renderer);
            aribcc_decoder_flush(self.decoder);
        }
    }

    fn decode_and_render(&mut self, pes: &[u8], pts_millis: jlong) -> Option<RenderedCaptionFrame> {
        if pes.is_empty()
            || pts_millis == ARIBCC_PTS_NOPTS
            || pts_millis < 0
            || self.viewport.is_none()
        {
            return None;
        }
        unsafe {
            let mut caption = AribccCaption::default();
            let decode_status = aribcc_decoder_decode(
                self.decoder,
                pes.as_ptr(),
                pes.len(),
                pts_millis,
                &mut caption,
            );
            if decode_status != ARIBCC_DECODE_STATUS_GOT_CAPTION {
                if !caption.text.is_null()
                    || !caption.regions.is_null()
                    || !caption.drcs_map.is_null()
                {
                    aribcc_caption_cleanup(&mut caption);
                }
                return None;
            }
            let Some((caption_pts, caption_duration)) =
                validate_decoded_caption_timing(caption.pts, pts_millis, caption.wait_duration)
            else {
                aribcc_caption_cleanup(&mut caption);
                return None;
            };
            let appended = aribcc_renderer_append_caption(self.renderer, &caption);
            aribcc_caption_cleanup(&mut caption);
            if !appended {
                return None;
            }

            let mut result = AribccRenderResult::default();
            let status = aribcc_renderer_render(self.renderer, caption_pts, &mut result);
            if status == ARIBCC_RENDER_STATUS_ERROR {
                cleanup_render_result_if_owned(&mut result);
                return None;
            }
            if !matches!(
                status,
                ARIBCC_RENDER_STATUS_NO_IMAGE
                    | ARIBCC_RENDER_STATUS_GOT_IMAGE
                    | ARIBCC_RENDER_STATUS_GOT_IMAGE_UNCHANGED
            ) {
                cleanup_render_result_if_owned(&mut result);
                return None;
            }

            let frame = copy_render_result(
                &result,
                caption_pts,
                caption_duration,
                status == ARIBCC_RENDER_STATUS_NO_IMAGE,
            );
            cleanup_render_result_if_owned(&mut result);
            frame
        }
    }
}

fn validate_decoded_caption_timing(
    decoded_pts: i64,
    authoritative_pts: i64,
    decoded_duration: i64,
) -> Option<(i64, i64)> {
    if decoded_pts == ARIBCC_PTS_NOPTS
        || decoded_pts != authoritative_pts
        || decoded_pts < 0
        || (decoded_duration != ARIBCC_DURATION_INDEFINITE && decoded_duration < 0)
    {
        return None;
    }
    Some((decoded_pts, decoded_duration))
}

unsafe fn cleanup_render_result_if_owned(result: &mut AribccRenderResult) {
    if !result.images.is_null() || result.image_count != 0 {
        aribcc_render_result_cleanup(result);
    }
}

fn copy_render_result(
    result: &AribccRenderResult,
    authoritative_pts: i64,
    decoded_duration: i64,
    no_image: bool,
) -> Option<RenderedCaptionFrame> {
    let pts_millis = if result.pts == ARIBCC_PTS_NOPTS {
        authoritative_pts
    } else {
        result.pts
    };
    if pts_millis < 0 || pts_millis != authoritative_pts {
        return None;
    }
    let duration_raw = if result.duration == ARIBCC_DURATION_INDEFINITE {
        decoded_duration
    } else {
        result.duration
    };
    let duration_millis = if duration_raw == ARIBCC_DURATION_INDEFINITE {
        None
    } else if duration_raw < 0 {
        return None;
    } else {
        Some(duration_raw)
    };
    if no_image {
        return Some(RenderedCaptionFrame {
            pts_millis,
            duration_millis,
            images: Vec::new(),
        });
    }
    if result.image_count == 0 || result.images.is_null() {
        return None;
    }
    let image_count = usize::try_from(result.image_count).ok()?;
    if image_count > MAX_IMAGES_PER_FRAME {
        return None;
    }
    let native_images = unsafe { slice::from_raw_parts(result.images, image_count) };
    let mut images = Vec::with_capacity(image_count);
    let mut total_bitmap_bytes = 0usize;
    for image in native_images {
        if image.pixel_format != ARIBCC_PIXELFORMAT_RGBA8888
            || image.width <= 0
            || image.height <= 0
            || image.stride < image.width.checked_mul(4)?
            || image.bitmap.is_null()
        {
            return None;
        }
        let required = usize::try_from(image.stride)
            .ok()?
            .checked_mul(usize::try_from(image.height).ok()?)?;
        total_bitmap_bytes = total_bitmap_bytes.checked_add(required)?;
        if required == 0
            || total_bitmap_bytes > MAX_FRAME_BITMAP_BYTES
            || required > image.bitmap_size as usize
        {
            return None;
        }
        images.push(RenderedCaptionImage {
            dst_x: image.dst_x,
            dst_y: image.dst_y,
            width: image.width,
            height: image.height,
            stride: image.stride,
            rgba8888: unsafe { slice::from_raw_parts(image.bitmap, required) }.to_vec(),
        });
    }
    Some(RenderedCaptionFrame {
        pts_millis,
        duration_millis,
        images,
    })
}

fn encode_rendered_frame(frame: RenderedCaptionFrame) -> Option<Vec<u8>> {
    let image_count = u32::try_from(frame.images.len()).ok()?;
    let bitmap_bytes = frame.images.iter().try_fold(0usize, |total, image| {
        total.checked_add(image.rgba8888.len())
    })?;
    if frame.images.len() > MAX_IMAGES_PER_FRAME || bitmap_bytes > MAX_FRAME_BITMAP_BYTES {
        return None;
    }
    let metadata_bytes = IMAGE_HEADER_BYTES.checked_mul(frame.images.len())?;
    let packet_bytes = FRAME_HEADER_BYTES
        .checked_add(metadata_bytes)?
        .checked_add(bitmap_bytes)?;
    let mut packet = Vec::new();
    packet.try_reserve_exact(packet_bytes).ok()?;
    packet.extend_from_slice(&frame.pts_millis.to_le_bytes());
    packet.extend_from_slice(&frame.duration_millis.unwrap_or(-1).to_le_bytes());
    packet.extend_from_slice(&image_count.to_le_bytes());
    for image in frame.images {
        let bitmap_bytes = u32::try_from(image.rgba8888.len()).ok()?;
        packet.extend_from_slice(&image.dst_x.to_le_bytes());
        packet.extend_from_slice(&image.dst_y.to_le_bytes());
        packet.extend_from_slice(&image.width.to_le_bytes());
        packet.extend_from_slice(&image.height.to_le_bytes());
        packet.extend_from_slice(&image.stride.to_le_bytes());
        packet.extend_from_slice(&bitmap_bytes.to_le_bytes());
        packet.extend_from_slice(&image.rgba8888);
    }
    Some(packet)
}

#[derive(Default)]
struct Registry {
    next_handle: jlong,
    engines: BTreeMap<jlong, CaptionEngine>,
}

impl Registry {
    fn next_handle(&mut self) -> jlong {
        self.next_handle = self.next_handle.checked_add(1).unwrap_or(1).max(1);
        self.next_handle
    }

    fn insert_engine(&mut self, engine: CaptionEngine) -> jlong {
        let handle = self.next_handle();
        self.engines.insert(handle, engine);
        handle
    }
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribCaptionRenderer_nativeCreateRenderer(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    data_component_id: jint,
    superimpose: jboolean,
) -> jlong {
    let Some(engine) = CaptionEngine::new(data_component_id, superimpose != 0) else {
        return 0;
    };
    registry()
        .lock()
        .map(|mut guard| guard.insert_engine(engine))
        .unwrap_or(0)
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribCaptionRenderer_nativeSetViewport(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    width: jint,
    height: jint,
) -> jboolean {
    registry()
        .lock()
        .ok()
        .and_then(|mut guard| {
            guard
                .engines
                .get_mut(&handle)
                .map(|engine| engine.set_viewport(width, height))
        })
        .unwrap_or(false) as jboolean
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribCaptionRenderer_nativeDecodeAndRender(
    env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    pes_data: JByteArray<'_>,
    pts_millis: jlong,
) -> jbyteArray {
    if handle == 0 || pts_millis == ARIBCC_PTS_NOPTS {
        return ptr::null_mut();
    }
    let Ok(bytes) = env.convert_byte_array(pes_data) else {
        return ptr::null_mut();
    };
    let Ok(mut guard) = registry().lock() else {
        return ptr::null_mut();
    };
    let frame = guard
        .engines
        .get_mut(&handle)
        .and_then(|engine| engine.decode_and_render(&bytes, pts_millis));
    drop(guard);
    let Some(packet) = frame.and_then(encode_rendered_frame) else {
        return ptr::null_mut();
    };
    env.byte_array_from_slice(&packet)
        .map(|array| array.into_raw())
        .unwrap_or(ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribCaptionRenderer_nativeFlush(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) {
    if let Ok(mut guard) = registry().lock() {
        if let Some(engine) = guard.engines.get_mut(&handle) {
            engine.flush();
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribCaptionRenderer_nativeReleaseRenderer(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) {
    if let Ok(mut guard) = registry().lock() {
        guard.engines.remove(&handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_c_api_constants_are_kept_exact() {
        assert_eq!(ARIBCC_CAPTIONTYPE_CAPTION, 0x80);
        assert_eq!(ARIBCC_CAPTIONTYPE_SUPERIMPOSE, 0x81);
        assert_eq!(ARIBCC_PROFILE_A, 0x0008);
        assert_eq!(ARIBCC_PROFILE_C, 0x0012);
        assert_eq!(ARIBCC_PTS_NOPTS, i64::MIN);
        assert_eq!(ARIBCC_DURATION_INDEFINITE, i64::MAX);
        assert_eq!(ARIBCC_PIXELFORMAT_RGBA8888, 0);
    }

    #[test]
    fn no_pts_is_never_renderer_input() {
        assert_eq!(ARIBCC_PTS_NOPTS, i64::MIN);
        assert_ne!(ARIBCC_PTS_NOPTS, 0);
    }

    #[test]
    fn decoded_caption_timing_requires_authoritative_pts_and_valid_duration() {
        assert_eq!(
            validate_decoded_caption_timing(90, 90, ARIBCC_DURATION_INDEFINITE),
            Some((90, ARIBCC_DURATION_INDEFINITE))
        );
        assert_eq!(
            validate_decoded_caption_timing(ARIBCC_PTS_NOPTS, 90, 10),
            None
        );
        assert_eq!(validate_decoded_caption_timing(91, 90, 10), None);
        assert_eq!(validate_decoded_caption_timing(90, 90, -1), None);
    }

    #[test]
    fn render_result_rejects_pts_mismatch_negative_duration_and_excessive_images() {
        let no_image = AribccRenderResult {
            pts: ARIBCC_PTS_NOPTS,
            duration: ARIBCC_DURATION_INDEFINITE,
            images: ptr::null_mut(),
            image_count: 0,
        };
        let frame = copy_render_result(&no_image, 100, 25, true).unwrap();
        assert_eq!(frame.pts_millis, 100);
        assert_eq!(frame.duration_millis, Some(25));
        assert!(frame.images.is_empty());

        let mismatched = AribccRenderResult {
            pts: 101,
            duration: ARIBCC_DURATION_INDEFINITE,
            images: ptr::null_mut(),
            image_count: 0,
        };
        assert!(copy_render_result(&mismatched, 100, 25, true).is_none());
        let negative_duration = AribccRenderResult {
            pts: 100,
            duration: -1,
            images: ptr::null_mut(),
            image_count: 0,
        };
        assert!(copy_render_result(&negative_duration, 100, 25, true).is_none());

        let excessive_images = AribccRenderResult {
            pts: 100,
            duration: 25,
            images: std::ptr::NonNull::<AribccImage>::dangling().as_ptr(),
            image_count: (MAX_IMAGES_PER_FRAME + 1) as u32,
        };
        assert!(copy_render_result(&excessive_images, 100, 25, false).is_none());
    }

    #[test]
    fn render_result_copies_rgba_and_rejects_aggregate_bitmap_budget_overflow() {
        let mut rgba = vec![1u8, 2, 3, 4];
        let mut image = AribccImage {
            width: 1,
            height: 1,
            stride: 4,
            dst_x: 2,
            dst_y: 3,
            pixel_format: ARIBCC_PIXELFORMAT_RGBA8888,
            bitmap: rgba.as_mut_ptr(),
            bitmap_size: rgba.len() as u32,
        };
        let result = AribccRenderResult {
            pts: 100,
            duration: 25,
            images: &mut image,
            image_count: 1,
        };
        let frame = copy_render_result(&result, 100, 25, false).unwrap();
        assert_eq!(frame.images[0].rgba8888, rgba);
        assert_eq!((frame.images[0].dst_x, frame.images[0].dst_y), (2, 3));

        image.width = 16_384;
        image.height = 4_097;
        image.stride = 65_536;
        image.bitmap = std::ptr::NonNull::<u8>::dangling().as_ptr();
        image.bitmap_size = u32::MAX;
        assert!(copy_render_result(&result, 100, 25, false).is_none());
    }

    #[test]
    fn rendered_frame_is_one_bounds_checked_little_endian_packet() {
        let frame = RenderedCaptionFrame {
            pts_millis: 100,
            duration_millis: Some(25),
            images: vec![RenderedCaptionImage {
                dst_x: 2,
                dst_y: 3,
                width: 1,
                height: 1,
                stride: 4,
                rgba8888: vec![0x11, 0x22, 0x33, 0x44],
            }],
        };
        let packet = encode_rendered_frame(frame).unwrap();
        assert_eq!(packet.len(), FRAME_HEADER_BYTES + IMAGE_HEADER_BYTES + 4);
        assert_eq!(&packet[0..8], &100i64.to_le_bytes());
        assert_eq!(&packet[8..16], &25i64.to_le_bytes());
        assert_eq!(&packet[16..20], &1u32.to_le_bytes());
        assert_eq!(&packet[20..24], &2i32.to_le_bytes());
        assert_eq!(&packet[24..28], &3i32.to_le_bytes());
        assert_eq!(&packet[40..44], &4u32.to_le_bytes());
        assert_eq!(&packet[44..48], &[0x11, 0x22, 0x33, 0x44]);
    }
}
