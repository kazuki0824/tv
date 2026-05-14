use jni::objects::{JByteArray, JObject};
use jni::sys::{jboolean, jint, jlong, jstring};
use jni::JNIEnv;
use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_longlong, c_void};
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};

const ARIBCC_CAPTIONTYPE_CAPTION: c_int = 0x80;
const ARIBCC_CAPTIONTYPE_SUPERIMPOSE: c_int = 0x81;
const ARIBCC_ENCODING_SCHEME_ARIB_STD_B24_JIS: c_int = 1;
const ARIBCC_DECODE_STATUS_GOT_CAPTION: c_int = 2;
const ARIBCC_PROFILE_A: c_int = 0x0008;
const ARIBCC_PROFILE_C: c_int = 0x0012;
const ARIBCC_PTS_NOPTS: c_longlong = i64::MIN;

#[repr(C)]
struct AribccCaption {
    caption_type: c_int,
    flags: c_int,
    iso6392_language_code: u32,
    text: *mut c_char,
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

type AribccContextAlloc = unsafe extern "C" fn() -> *mut c_void;
type AribccContextFree = unsafe extern "C" fn(*mut c_void);
type AribccDecoderAlloc = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type AribccDecoderFree = unsafe extern "C" fn(*mut c_void);
type AribccDecoderInitialize = unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, c_int) -> bool;
type AribccDecoderSetProfile = unsafe extern "C" fn(*mut c_void, c_int);
type AribccDecoderSetCaptionType = unsafe extern "C" fn(*mut c_void, c_int);
type AribccDecoderDecode = unsafe extern "C" fn(*mut c_void, *const u8, usize, c_longlong, *mut AribccCaption) -> c_int;
type AribccCaptionCleanup = unsafe extern "C" fn(*mut AribccCaption);

#[link(name = "dl")]
extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
}

struct AribccApi {
    lib: *mut c_void,
    context_alloc: AribccContextAlloc,
    context_free: AribccContextFree,
    decoder_alloc: AribccDecoderAlloc,
    decoder_free: AribccDecoderFree,
    decoder_initialize: AribccDecoderInitialize,
    decoder_set_profile: AribccDecoderSetProfile,
    decoder_set_caption_type: AribccDecoderSetCaptionType,
    decoder_decode: AribccDecoderDecode,
    caption_cleanup: AribccCaptionCleanup,
}

unsafe impl Send for AribccApi {}
unsafe impl Sync for AribccApi {}

impl Drop for AribccApi {
    fn drop(&mut self) {
        unsafe { let _ = dlclose(self.lib); }
    }
}

impl AribccApi {
    fn load_symbol(handle: *mut c_void, name: &str) -> Option<*mut c_void> {
        let c_name = CString::new(name).ok()?;
        let symbol = unsafe { dlsym(handle, c_name.as_ptr()) };
        if symbol.is_null() { None } else { Some(symbol) }
    }

    unsafe fn load_fn<T: Copy>(handle: *mut c_void, name: &str) -> Option<T> {
        let symbol = Self::load_symbol(handle, name)?;
        Some(std::mem::transmute_copy::<*mut c_void, T>(&symbol))
    }

    fn load() -> Option<Arc<Self>> {
        static API: OnceLock<Option<Arc<AribccApi>>> = OnceLock::new();
        API.get_or_init(|| unsafe {
            let lib_name = CString::new("libaribcaption.so").ok()?;
            let lib = dlopen(lib_name.as_ptr(), 2);
            if lib.is_null() { return None; }
            let api = AribccApi {
                lib,
                context_alloc: Self::load_fn(lib, "aribcc_context_alloc")?,
                context_free: Self::load_fn(lib, "aribcc_context_free")?,
                decoder_alloc: Self::load_fn(lib, "aribcc_decoder_alloc")?,
                decoder_free: Self::load_fn(lib, "aribcc_decoder_free")?,
                decoder_initialize: Self::load_fn(lib, "aribcc_decoder_initialize")?,
                decoder_set_profile: Self::load_fn(lib, "aribcc_decoder_set_profile")?,
                decoder_set_caption_type: Self::load_fn(lib, "aribcc_decoder_set_caption_type")?,
                decoder_decode: Self::load_fn(lib, "aribcc_decoder_decode")?,
                caption_cleanup: Self::load_fn(lib, "aribcc_caption_cleanup")?,
            };
            Some(Arc::new(api))
        }).clone()
    }
}

struct CaptionDecoder {
    api: Arc<AribccApi>,
    context: *mut c_void,
    decoder: *mut c_void,
}

unsafe impl Send for CaptionDecoder {}

impl Drop for CaptionDecoder {
    fn drop(&mut self) {
        unsafe {
            if !self.decoder.is_null() {
                (self.api.decoder_free)(self.decoder);
                self.decoder = ptr::null_mut();
            }
            if !self.context.is_null() {
                (self.api.context_free)(self.context);
                self.context = ptr::null_mut();
            }
        }
    }
}

impl CaptionDecoder {
    fn new() -> Option<Self> {
        let api = AribccApi::load()?;
        unsafe {
            let context = (api.context_alloc)();
            if context.is_null() { return None; }
            let decoder = (api.decoder_alloc)(context);
            if decoder.is_null() {
                (api.context_free)(context);
                return None;
            }
            let initialized = (api.decoder_initialize)(
                decoder,
                ARIBCC_ENCODING_SCHEME_ARIB_STD_B24_JIS,
                ARIBCC_CAPTIONTYPE_CAPTION,
                ARIBCC_PROFILE_A,
                1,
            );
            if !initialized {
                (api.decoder_free)(decoder);
                (api.context_free)(context);
                return None;
            }
            Some(Self { api, context, decoder })
        }
    }

    fn decode_pes(&mut self, pes: &[u8], pts_millis: jlong, data_component_id: jint, superimpose: bool) -> Option<String> {
        if pes.is_empty() || self.decoder.is_null() { return None; }
        let profile = match data_component_id {
            0x0012 => ARIBCC_PROFILE_C,
            _ => ARIBCC_PROFILE_A,
        };
        let caption_type = if superimpose { ARIBCC_CAPTIONTYPE_SUPERIMPOSE } else { ARIBCC_CAPTIONTYPE_CAPTION };
        let pts = if pts_millis == i64::MIN { ARIBCC_PTS_NOPTS } else { pts_millis as c_longlong };
        unsafe {
            (self.api.decoder_set_profile)(self.decoder, profile);
            (self.api.decoder_set_caption_type)(self.decoder, caption_type);
            let mut caption = AribccCaption {
                caption_type,
                flags: 0,
                iso6392_language_code: 0,
                text: ptr::null_mut(),
                regions: ptr::null_mut(),
                region_count: 0,
                drcs_map: ptr::null_mut(),
                pts,
                wait_duration: 0,
                plane_width: 0,
                plane_height: 0,
                has_builtin_sound: false,
                builtin_sound_id: 0,
            };
            let status = (self.api.decoder_decode)(self.decoder, pes.as_ptr(), pes.len(), pts, &mut caption);
            if status != ARIBCC_DECODE_STATUS_GOT_CAPTION || caption.text.is_null() {
                if !caption.text.is_null() || !caption.regions.is_null() || !caption.drcs_map.is_null() {
                    (self.api.caption_cleanup)(&mut caption);
                }
                return None;
            }
            let text = CStr::from_ptr(caption.text).to_string_lossy().into_owned();
            (self.api.caption_cleanup)(&mut caption);
            if text.trim().is_empty() { None } else { Some(text) }
        }
    }
}

#[derive(Default)]
struct CaptionRegistry {
    next_handle: jlong,
    decoders: BTreeMap<jlong, CaptionDecoder>,
}

impl CaptionRegistry {
    fn insert(&mut self, decoder: CaptionDecoder) -> jlong {
        self.next_handle = self.next_handle.saturating_add(1).max(1);
        let handle = self.next_handle;
        self.decoders.insert(handle, decoder);
        handle
    }

    fn remove(&mut self, handle: jlong) -> bool {
        self.decoders.remove(&handle).is_some()
    }
}

static CAPTION_REGISTRY: OnceLock<Mutex<CaptionRegistry>> = OnceLock::new();

fn caption_registry() -> &'static Mutex<CaptionRegistry> {
    CAPTION_REGISTRY.get_or_init(|| Mutex::new(CaptionRegistry::default()))
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribCaptionRenderer_nativeCreateRenderer(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jlong {
    let Some(decoder) = CaptionDecoder::new() else { return 0; };
    match caption_registry().lock() {
        Ok(mut guard) => guard.insert(decoder),
        Err(_) => 0,
    }
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribCaptionRenderer_nativeReleaseRenderer(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) {
    if handle == 0 { return; }
    if let Ok(mut guard) = caption_registry().lock() {
        let _ = guard.remove(handle);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_maleicacid_tvinput_aribsi_NativeAribCaptionRenderer_nativeDecodePes(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    pes_data: JByteArray<'_>,
    pts_millis: jlong,
    data_component_id: jint,
    superimpose: jboolean,
) -> jstring {
    if handle == 0 { return ptr::null_mut(); }
    let Ok(bytes) = env.convert_byte_array(pes_data) else { return ptr::null_mut(); };
    let text = match caption_registry().lock() {
        Ok(mut guard) => guard.decoders.get_mut(&handle)
            .and_then(|decoder| decoder.decode_pes(&bytes, pts_millis, data_component_id, superimpose != 0)),
        Err(_) => None,
    };
    match text {
        Some(value) => match env.new_string(value) {
            Ok(s) => s.into_raw(),
            Err(_) => ptr::null_mut(),
        },
        None => ptr::null_mut(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aribcc_constants_match_public_c_api_values() {
        assert_eq!(ARIBCC_CAPTIONTYPE_CAPTION, 0x80);
        assert_eq!(ARIBCC_CAPTIONTYPE_SUPERIMPOSE, 0x81);
        assert_eq!(ARIBCC_ENCODING_SCHEME_ARIB_STD_B24_JIS, 1);
        assert_eq!(ARIBCC_DECODE_STATUS_GOT_CAPTION, 2);
        assert_eq!(ARIBCC_PROFILE_A, 0x0008);
        assert_eq!(ARIBCC_PROFILE_C, 0x0012);
        assert_eq!(ARIBCC_PTS_NOPTS, i64::MIN);
    }

    #[test]
    fn no_pts_sentinel_uses_libaribcaption_public_value() {
        let pts_millis: jlong = i64::MIN;
        let pts = if pts_millis == i64::MIN { ARIBCC_PTS_NOPTS } else { pts_millis as c_longlong };
        assert_eq!(pts, i64::MIN);
    }
}
