use std::ffi::{c_int, c_void};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use super::{Api, Mpv};

const PARAM_INVALID: c_int = 0;
const PARAM_API_TYPE: c_int = 1;
const PARAM_SW_SIZE: c_int = 17;
const PARAM_SW_FORMAT: c_int = 18;
const PARAM_SW_STRIDE: c_int = 19;
const PARAM_SW_POINTER: c_int = 20;
const UPDATE_FRAME: u64 = 1;

#[repr(C)]
pub struct Param {
    kind: c_int,
    data: *mut c_void,
}

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

#[derive(Default)]
struct State {
    stop: bool,
    wake: bool,
    size: (u32, u32),
    frame: Option<Frame>,
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
}

struct Ctx(*mut c_void);
unsafe impl Send for Ctx {}

pub struct Renderer {
    api: Arc<Api>,
    ctx: *mut c_void,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

extern "C" fn on_update(shared: *mut c_void) {
    let shared = unsafe { &*(shared as *const Shared) };
    if let Ok(mut s) = shared.state.lock() {
        s.wake = true;
        shared.cv.notify_one();
    }
}

impl Renderer {
    pub fn new(mpv: &Mpv) -> Result<Self, String> {
        let api = mpv.api.clone();
        let mut ctx: *mut c_void = std::ptr::null_mut();
        let mut params = [
            Param {
                kind: PARAM_API_TYPE,
                data: c"sw".as_ptr() as *mut c_void,
            },
            Param {
                kind: PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        let code = unsafe { (api.render_create)(&mut ctx, mpv.handle, params.as_mut_ptr()) };
        if code < 0 || ctx.is_null() {
            return Err(format!("libmpv cannot render frames: {}", api.err(code)));
        }
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            cv: Condvar::new(),
        });
        unsafe {
            (api.render_set_callback)(ctx, Some(on_update), Arc::as_ptr(&shared) as *mut c_void)
        };
        let thread = {
            let (api, shared, ctx) = (api.clone(), shared.clone(), Ctx(ctx));
            std::thread::Builder::new()
                .name("mpv-render".into())
                .spawn(move || run(&api, ctx, &shared))
                .map_err(|e| e.to_string())?
        };
        Ok(Self {
            api,
            ctx,
            shared,
            thread: Some(thread),
        })
    }

    pub fn set_size(&self, size: (u32, u32)) {
        if let Ok(mut s) = self.shared.state.lock() {
            if s.size != size {
                s.size = size;
                self.shared.cv.notify_one();
            }
        }
    }

    pub fn take(&self) -> Option<Frame> {
        self.shared.state.lock().ok()?.frame.take()
    }
}

fn run(api: &Api, ctx: Ctx, shared: &Shared) {
    let ctx = ctx.0;
    let mut drawn = (0, 0);
    loop {
        let size = {
            let Ok(s) = shared.state.lock() else { return };
            let Ok(mut s) = shared
                .cv
                .wait_while(s, |s| !s.stop && !s.wake && s.size == drawn)
            else {
                return;
            };
            if s.stop {
                return;
            }
            s.wake = false;
            s.size
        };
        let flags = unsafe { (api.render_update)(ctx) };
        if size.0 == 0 || size.1 == 0 {
            drawn = size;
            continue;
        }
        if flags & UPDATE_FRAME == 0 && size == drawn {
            continue;
        }
        drawn = size;
        let (w, h) = (size.0 as usize, size.1 as usize);
        let mut buf = vec![0u8; w * h * 4];
        let mut dims = [size.0 as c_int, size.1 as c_int];
        let mut stride: usize = w * 4;
        let mut params = [
            Param {
                kind: PARAM_SW_SIZE,
                data: dims.as_mut_ptr().cast(),
            },
            Param {
                kind: PARAM_SW_FORMAT,
                data: c"bgr0".as_ptr() as *mut c_void,
            },
            Param {
                kind: PARAM_SW_STRIDE,
                data: (&mut stride as *mut usize).cast(),
            },
            Param {
                kind: PARAM_SW_POINTER,
                data: buf.as_mut_ptr().cast(),
            },
            Param {
                kind: PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        let code = unsafe { (api.render)(ctx, params.as_mut_ptr()) };
        if code < 0 {
            tracing::debug!("mpv render: {}", api.err(code));
            continue;
        }
        for px in buf.chunks_exact_mut(4) {
            px[3] = 255;
        }
        if let Ok(mut s) = shared.state.lock() {
            s.frame = Some(Frame {
                width: size.0,
                height: size.1,
                bgra: buf,
            });
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        if let Ok(mut s) = self.shared.state.lock() {
            s.stop = true;
            self.shared.cv.notify_one();
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        unsafe { (self.api.render_free)(self.ctx) };
    }
}
