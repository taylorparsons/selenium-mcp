use crate::error::AppError;
use crate::session::SessionSlot;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::task::JoinHandle;

pub struct ActiveRecording {
    pub run_dir: PathBuf,
    pub frames_dir: PathBuf,
    stop: Arc<AtomicBool>,
    frame_count: Arc<AtomicU32>,
    timestamps: Arc<Mutex<Vec<f64>>>,
    start: Instant,
    task: JoinHandle<()>,
}

#[derive(Clone, Default)]
pub struct RecordingSlot(Arc<Mutex<Option<ActiveRecording>>>);

/// ffmpeg's concat demuxer only honors a `duration` on the file line *before*
/// it — the standard workaround is to repeat the final file once more with no
/// duration after it. This is what lets record_stop encode using each frame's
/// *real* capture-to-capture gap instead of assuming a fixed framerate, which
/// is what keeps MP4 pacing correct when a page makes one frame take longer
/// than 1/fps to capture (design.md §10 risk; spec.md's FR-007 edge case).
pub fn build_concat_file(frame_count: u32, timestamps: &[f64], stop_elapsed: f64) -> String {
    let mut out = String::from("ffconcat version 1.0\n");
    for i in 0..frame_count as usize {
        let name = format!("{:05}.png", i + 1);
        let duration = if i + 1 < timestamps.len() {
            timestamps[i + 1] - timestamps[i]
        } else {
            stop_elapsed - timestamps[i]
        };
        out.push_str(&format!("file '{name}'\nduration {duration:.6}\n"));
    }
    if frame_count > 0 {
        out.push_str(&format!("file '{:05}.png'\n", frame_count));
    }
    out
}

const MAX_FPS: u32 = 5;
const DEFAULT_FPS: u32 = 3;

pub fn clamp_fps(requested: Option<u32>) -> Result<u32, AppError> {
    match requested {
        None => Ok(DEFAULT_FPS),
        Some(fps) if (1..=MAX_FPS).contains(&fps) => Ok(fps),
        Some(fps) => Err(AppError::InvalidTarget(format!(
            "fps must be between 1 and {MAX_FPS}, got {fps}"
        ))),
    }
}

impl RecordingSlot {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }

    /// Captures frame 1 synchronously (this both validates a session exists
    /// and avoids a dead first tick), then spawns the background capture
    /// loop for every frame after it.
    pub async fn start(
        &self,
        session: SessionSlot,
        run_dir: PathBuf,
        frames_dir: PathBuf,
        fps: u32,
    ) -> Result<(), AppError> {
        {
            let guard = self.0.lock().unwrap();
            if guard.is_some() {
                return Err(AppError::RecordingAlreadyActive);
            }
        }

        let first_frame = crate::project::managed_path(&frames_dir, "00001", ".png")?;
        std::fs::create_dir_all(&frames_dir)?;
        session
            .with_driver(|driver| {
                let path = first_frame.clone();
                async move { driver.screenshot(&path).await.map_err(AppError::from) }
            })
            .await?;

        let frame_count = Arc::new(AtomicU32::new(1));
        let timestamps = Arc::new(Mutex::new(vec![0.0]));
        let stop = Arc::new(AtomicBool::new(false));
        let start = Instant::now();

        let task = tokio::spawn(capture_loop(
            session,
            frames_dir.clone(),
            fps,
            stop.clone(),
            frame_count.clone(),
            timestamps.clone(),
            start,
        ));

        let mut guard = self.0.lock().unwrap();
        *guard = Some(ActiveRecording {
            run_dir,
            frames_dir,
            stop,
            frame_count,
            timestamps,
            start,
            task,
        });
        Ok(())
    }

    pub async fn stop(&self) -> Result<(PathBuf, PathBuf, u32, Vec<f64>, f64), AppError> {
        let recording = self
            .0
            .lock()
            .unwrap()
            .take()
            .ok_or(AppError::NoActiveRecording)?;

        recording.stop.store(true, Ordering::Relaxed);
        let _ = recording.task.await;

        let stop_elapsed = recording.start.elapsed().as_secs_f64();
        let frame_count = recording.frame_count.load(Ordering::Relaxed);
        let timestamps = recording.timestamps.lock().unwrap().clone();
        Ok((
            recording.run_dir,
            recording.frames_dir,
            frame_count,
            timestamps,
            stop_elapsed,
        ))
    }
}

async fn capture_loop(
    session: SessionSlot,
    frames_dir: PathBuf,
    fps: u32,
    stop: Arc<AtomicBool>,
    frame_count: Arc<AtomicU32>,
    timestamps: Arc<Mutex<Vec<f64>>>,
    start: Instant,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs_f64(1.0 / fps as f64));
    // Burst (the default) fires ticks back-to-back to "catch up" if a
    // screenshot outlasts 1/fps — at 5 fps a 200ms screenshot (design.md's
    // own risk estimate) already exceeds the 200ms period, so Burst would
    // both starve the with_driver mutex other tools need and, on a
    // fixed-framerate encode, play the video too fast. Skip just drops the
    // missed ticks instead.
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // First tick fires immediately; frame 1 was already captured synchronously
    // in start(), so this one is thrown away to avoid double-capturing t=0.
    interval.tick().await;

    loop {
        interval.tick().await;
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let index = frame_count.fetch_add(1, Ordering::Relaxed) + 1;
        let elapsed = start.elapsed().as_secs_f64();
        let path = match crate::project::managed_path(&frames_dir, &format!("{index:05}"), ".png") {
            Ok(path) => path,
            Err(_) => return,
        };
        let captured = session
            .with_driver(|driver| {
                let path = path.clone();
                async move { driver.screenshot(&path).await.map_err(AppError::from) }
            })
            .await;
        match captured {
            Ok(()) => timestamps.lock().unwrap().push(elapsed),
            Err(_) => return, // session ended (or errored) — nothing left to record
        }
        if stop.load(Ordering::Relaxed) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_concat_file_emits_one_duration_per_frame_gap_plus_a_repeated_last_line() {
        let listing = build_concat_file(3, &[0.0, 0.5, 1.2], 1.6);

        let expected = "ffconcat version 1.0\n\
file '00001.png'\n\
duration 0.500000\n\
file '00002.png'\n\
duration 0.700000\n\
file '00003.png'\n\
duration 0.400000\n\
file '00003.png'\n";
        assert_eq!(listing, expected);
    }

    #[test]
    fn build_concat_file_handles_a_single_frame() {
        let listing = build_concat_file(1, &[0.0], 0.25);

        let expected = "ffconcat version 1.0\n\
file '00001.png'\n\
duration 0.250000\n\
file '00001.png'\n";
        assert_eq!(listing, expected);
    }
}
