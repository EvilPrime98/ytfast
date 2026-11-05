use std::ffi::{CStr, CString, c_char, c_int, c_ulong, c_void};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use libloading::Library;

#[cfg(windows)]
mod playback;
#[cfg(windows)]
mod render;

#[cfg(windows)]
pub use playback::Playback;

#[cfg(not(windows))]
mod unsupported {
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;

    use gpui::RenderImage;

    pub struct Playback;

    impl Playback {
        pub fn new(_: &str) -> Result<Self, String> {
            Err("video mode is only available on Windows for now".into())
        }
        pub fn load(&mut self, _: &Path, _: Duration, _: bool) -> Result<(), String> {
            Ok(())
        }
        pub fn seek(&self, _: Duration) {}
        pub fn set_size(&mut self, _: (u32, u32)) {}
        pub fn drive(&mut self, _: bool, _: Duration, _: f32) {}
        pub fn take_frame(&mut self) -> Option<Arc<RenderImage>> {
            None
        }
    }
}
#[cfg(not(windows))]
pub use unsupported::Playback;

type Handle = *mut c_void;

const FORMAT_DOUBLE: c_int = 5;

struct Api {
    create: unsafe extern "C" fn() -> Handle,
    initialize: unsafe extern "C" fn(Handle) -> c_int,
    set_option_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    set_property_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    command: unsafe extern "C" fn(Handle, *const *const c_char) -> c_int,
    get_property: unsafe extern "C" fn(Handle, *const c_char, c_int, *mut c_void) -> c_int,
    destroy: unsafe extern "C" fn(Handle),
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    api_version: unsafe extern "C" fn() -> c_ulong,
    render_create: unsafe extern "C" fn(*mut Handle, Handle, *mut render::Param) -> c_int,
    render_set_callback:
        unsafe extern "C" fn(Handle, Option<extern "C" fn(*mut c_void)>, *mut c_void),
    render_update: unsafe extern "C" fn(Handle) -> u64,
    render: unsafe extern "C" fn(Handle, *mut render::Param) -> c_int,
    render_free: unsafe extern "C" fn(Handle),
    _lib: Library,
}

const NOT_FOUND: &str =
    "libmpv was not found. Put libmpv-2.dll next to YTFast or set its path in Settings.";

fn candidates(path: &str) -> Vec<PathBuf> {
    let path = path.trim();
    if path.is_empty() {
        return vec!["libmpv-2.dll".into(), "mpv-2.dll".into()];
    }
    let p = PathBuf::from(path);
    if p.is_dir() {
        vec![p.join("libmpv-2.dll"), p.join("mpv-2.dll")]
    } else {
        vec![p]
    }
}

impl Api {
    fn load(path: &str) -> Result<Self, String> {
        let lib = candidates(path)
            .into_iter()
            .find_map(|c| unsafe { Library::new(c).ok() })
            .ok_or(NOT_FOUND)?;
        macro_rules! sym {
            ($name:literal) => {
                *unsafe { lib.get(concat!($name, "\0").as_bytes()) }
                    .map_err(|e| format!("this is not a usable libmpv ({e})"))?
            };
        }
        Ok(Self {
            create: sym!("mpv_create"),
            initialize: sym!("mpv_initialize"),
            set_option_string: sym!("mpv_set_option_string"),
            set_property_string: sym!("mpv_set_property_string"),
            command: sym!("mpv_command"),
            get_property: sym!("mpv_get_property"),
            destroy: sym!("mpv_terminate_destroy"),
            error_string: sym!("mpv_error_string"),
            api_version: sym!("mpv_client_api_version"),
            render_create: sym!("mpv_render_context_create"),
            render_set_callback: sym!("mpv_render_context_set_update_callback"),
            render_update: sym!("mpv_render_context_update"),
            render: sym!("mpv_render_context_render"),
            render_free: sym!("mpv_render_context_free"),
            _lib: lib,
        })
    }

    fn err(&self, code: c_int) -> String {
        unsafe { CStr::from_ptr((self.error_string)(code)) }
            .to_string_lossy()
            .into_owned()
    }
}

pub fn check(path: &str) -> Result<String, String> {
    if !cfg!(windows) {
        return Err("video mode is only available on Windows for now".into());
    }
    let api = Api::load(path)?;
    let v = unsafe { (api.api_version)() };
    Ok(format!("libmpv {}.{}", v >> 16, v & 0xffff))
}

fn c(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| "unexpected character in a path".to_string())
}

pub struct Mpv {
    api: Arc<Api>,
    handle: Handle,
}

impl Mpv {
    pub fn new(lib_path: &str) -> Result<Self, String> {
        let api = Arc::new(Api::load(lib_path)?);
        let handle = unsafe { (api.create)() };
        if handle.is_null() {
            return Err("libmpv could not be created".into());
        }
        let mpv = Self { api, handle };
        let options = [
            ("vo", "libmpv".to_string()),
            ("terminal", "no".into()),
            ("config", "no".into()),
            ("load-scripts", "no".into()),
            ("ytdl", "no".into()),
            ("osc", "no".into()),
            ("osd-level", "0".into()),
            ("input-default-bindings", "no".into()),
            ("input-vo-keyboard", "no".into()),
            ("cursor-autohide", "no".into()),
            ("aid", "no".into()),
            ("hwdec", "auto-copy-safe".into()),
            ("keep-open", "yes".into()),
            ("idle", "yes".into()),
            ("hr-seek", "yes".into()),
        ];
        for (name, value) in options {
            let (n, v) = (c(name)?, c(&value)?);
            let code = unsafe { (mpv.api.set_option_string)(mpv.handle, n.as_ptr(), v.as_ptr()) };
            if code < 0 {
                tracing::debug!("mpv option {name}: {}", mpv.api.err(code));
            }
        }
        let code = unsafe { (mpv.api.initialize)(mpv.handle) };
        if code < 0 {
            return Err(format!("libmpv failed to start: {}", mpv.api.err(code)));
        }
        Ok(mpv)
    }

    fn set(&self, name: &str, value: &str) -> Result<(), String> {
        let (n, v) = (c(name)?, c(value)?);
        let code = unsafe { (self.api.set_property_string)(self.handle, n.as_ptr(), v.as_ptr()) };
        (code >= 0).then_some(()).ok_or_else(|| self.api.err(code))
    }

    fn command(&self, args: &[&str]) -> Result<(), String> {
        let owned: Vec<CString> = args.iter().map(|a| c(a)).collect::<Result<_, _>>()?;
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|a| a.as_ptr()).collect();
        ptrs.push(std::ptr::null());
        let code = unsafe { (self.api.command)(self.handle, ptrs.as_ptr()) };
        (code >= 0).then_some(()).ok_or_else(|| self.api.err(code))
    }

    pub fn load(&self, file: &Path, start: Duration, paused: bool) -> Result<(), String> {
        self.set("start", &format!("{:.3}", start.as_secs_f64()))?;
        self.set("pause", if paused { "yes" } else { "no" })?;
        self.command(&["loadfile", &file.to_string_lossy(), "replace"])
    }

    pub fn seek(&self, to: Duration) {
        let _ = self.command(&[
            "seek",
            &format!("{:.3}", to.as_secs_f64()),
            "absolute+exact",
        ]);
    }

    pub fn set_pause(&self, paused: bool) {
        let _ = self.set("pause", if paused { "yes" } else { "no" });
    }

    pub fn set_speed(&self, speed: f64) {
        let _ = self.set("speed", &format!("{speed:.4}"));
    }

    pub fn time_pos(&self) -> Option<f64> {
        let name = c("time-pos").ok()?;
        let mut out: f64 = 0.0;
        let code = unsafe {
            (self.api.get_property)(
                self.handle,
                name.as_ptr(),
                FORMAT_DOUBLE,
                (&mut out as *mut f64).cast(),
            )
        };
        (code >= 0).then_some(out)
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        unsafe { (self.api.destroy)(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_library_is_a_readable_error() {
        let err = check("C:/definitely/not/here/libmpv-2.dll").unwrap_err();
        assert!(err.contains("libmpv"));
    }

    #[test]
    fn a_folder_means_the_dll_inside_it() {
        let dir = std::env::temp_dir();
        let found = candidates(dir.to_str().unwrap());
        assert_eq!(found[0], dir.join("libmpv-2.dll"));
        assert_eq!(candidates("")[0], PathBuf::from("libmpv-2.dll"));
    }

    #[test]
    #[ignore]
    fn mpv_loads_and_starts() {
        let path = std::env::var("YTFAST_LIBMPV").unwrap_or_default();
        assert!(check(&path).unwrap().starts_with("libmpv"));
        let mpv = Mpv::new(&path).unwrap();
        assert!(mpv.time_pos().is_none());
    }
}
