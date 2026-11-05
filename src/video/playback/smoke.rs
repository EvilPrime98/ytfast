use std::time::Duration;

use super::Playback;

#[test]
#[ignore]
fn renders_frames_into_a_buffer() {
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

    let mut pb = Playback::new(&lib).unwrap();
    pb.set_size((640, 360));
    pb.load(&clip, Duration::from_secs(1), false).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let first = pb.mpv.time_pos().expect("a position once loaded");
    assert!(first >= 1.0, "started at {first}");
    assert!(pb.take_frame().is_some(), "a frame was rendered");
    pb.drive(true, Duration::from_secs_f64(first + 0.2), 1.0);
    assert!(pb.speed > 1.0, "speed {}", pb.speed);
    pb.drive(true, Duration::from_secs(4), 1.0);
    std::thread::sleep(Duration::from_millis(600));
    let after = pb.mpv.time_pos().unwrap();
    assert!(after >= 3.9, "jumped to {after}");
    pb.drive(false, Duration::from_secs(4), 1.0);
    drop(pb);
    let _ = std::fs::remove_file(clip);
}
