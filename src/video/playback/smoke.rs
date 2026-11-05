use std::ptr::null_mut;
use std::time::Duration;

use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

use super::Playback;

#[test]
#[ignore]
fn plays_in_a_child_window() {
    let lib = std::env::var("YTFAST_LIBMPV").unwrap_or_default();
    let clip = std::env::temp_dir().join(format!("ytfast-mpv-{}.mp4", std::process::id()));
    let made = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg("testsrc=size=640x360:rate=30:duration=6")
        .args(["-pix_fmt", "yuv420p"])
        .arg(&clip)
        .status()
        .unwrap();
    assert!(made.success());

    let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    let top = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            0,
            0,
            800,
            600,
            null_mut(),
            null_mut(),
            null_mut(),
            std::ptr::null(),
        )
    };
    assert!(!top.is_null());

    let mut pb = Playback::new(&lib, top as isize).unwrap();
    pb.place((10, 10, 640, 360));
    pb.load(&clip, Duration::from_secs(1), false).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let first = pb.mpv.time_pos().expect("a position once loaded");
    assert!(first >= 1.0, "started at {first}");
    pb.drive(true, Duration::from_secs_f64(first + 0.2), 1.0);
    assert!(pb.speed > 1.0, "speed {}", pb.speed);
    pb.drive(true, Duration::from_secs(4), 1.0);
    std::thread::sleep(Duration::from_millis(600));
    let after = pb.mpv.time_pos().unwrap();
    assert!(after >= 3.9, "jumped to {after}");
    pb.drive(false, Duration::from_secs(4), 1.0);
    let _ = pb.poll_input();
    drop(pb);
    unsafe { DestroyWindow(top) };
    let _ = std::fs::remove_file(clip);
}
