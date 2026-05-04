use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use anyhow::Result;
use slint::{ComponentHandle, SharedString};
use walkdir::WalkDir;

use crate::{AppWindow, Args, State, Track, utils};

pub struct Application {
    weak: slint::Weak<AppWindow>,
    sink: Rc<rodio::Player>,
    timer: slint::Timer,

    track_i: RefCell<usize>,
    tracks: Vec<Track>,
}

impl Application {
    pub fn build(
        window: &AppWindow,
        sink: Rc<rodio::Player>,
        args: Args,
    ) -> Result<Rc<Self>> {
        let mut tracks = Vec::new();

        println!("Scanning for music files in {}", args.path);
        for entry in WalkDir::new(&args.path) {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("Error reading entry: {}", e);
                    continue;
                },
            };

            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    eprintln!(
                        "Error reading metadata for {}: {}",
                        entry.path().display(),
                        e
                    );
                    continue;
                },
            };

            let ext =
                entry.path().extension().and_then(|s| s.to_str()).unwrap_or("");

            if metadata.is_file()
                && matches!(
                    ext,
                    "mp3" | "flac" | "ogg" | "m4a" | "opus" | "wav"
                )
            {
                tracks.push(Track {
                    artist: entry
                        .path()
                        .parent()
                        .unwrap()
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned()
                        .into(),
                    title: entry
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                        .into(),
                    path: entry.path().to_string_lossy().into_owned().into(),
                });
            }
        }
        println!("Found {} tracks", tracks.len());

        Ok(Rc::new(Self {
            weak: window.as_weak(),
            sink,
            timer: slint::Timer::default(),
            track_i: RefCell::new(0),
            tracks,
        }))
    }

    fn window(&self) -> AppWindow { self.weak.upgrade().expect("window died") }

    fn track(&self) -> Option<&Track> {
        self.tracks.get(*self.track_i.borrow())
    }

    fn poll_playback(&self) {
        let pos = self.sink.get_pos().as_secs_f64();
        let window = self.window();

        window.global::<State>().set_current_time_text(SharedString::from(
            utils::format_duration_secs(pos),
        ));

        window.global::<State>().set_playing(!self.sink.is_paused());
    }

    fn poll_advance(&self) {
        if !self.sink.is_paused() && self.sink.empty() {
            let cur_i = *self.track_i.borrow();
            *self.track_i.borrow_mut() = (cur_i + 1) % self.tracks.len();
            if let Some(track) = self.track() {
                self.set_playback_to(track);
            } else {
                self.window()
                    .global::<State>()
                    .set_current_time_text(SharedString::from("0:00"));
            }
        }
    }

    fn set_playback_to(&self, track: &Track) {
        let file = match std::fs::File::open(track.path.as_str()) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Error opening file {}: {}", track.path, e);
                return;
            },
        };

        let source = match rodio::Decoder::new(std::io::BufReader::new(file)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error decoding file {}: {}", track.path, e);
                return;
            },
        };

        if !self.sink.is_paused() {
            self.sink.stop();
        }
        self.sink.append(source);
        self.sink.play();
        {
            let window = self.window();
            window.global::<State>().set_track(track.clone());
        }
    }

    fn register_play_pause(self: &Rc<Self>) {
        let app = self.clone();
        self.window().global::<State>().on_play(move || {
            if app.sink.empty() {
                if let Some(track) = app.track() {
                    app.set_playback_to(track);
                }
                return;
            }

            if app.sink.is_paused() {
                app.sink.play()
            } else {
                app.sink.pause()
            }
        });
    }

    fn register_next(self: &Rc<Self>) {
        let app = self.clone();
        self.window().global::<State>().on_next(move || {
            let cur_i = *app.track_i.borrow();
            *app.track_i.borrow_mut() =
                (cur_i + app.tracks.len() + 1) % app.tracks.len();
            if let Some(track) = app.track() {
                app.set_playback_to(track);
            }
        });
    }

    fn register_prev(self: &Rc<Self>) {
        let app = self.clone();
        self.window().global::<State>().on_prev(move || {
            let cur_i = *app.track_i.borrow();
            *app.track_i.borrow_mut() =
                (cur_i + app.tracks.len() - 1) % app.tracks.len();
            if let Some(track) = app.track() {
                app.set_playback_to(track);
            }
        });
    }

    fn register_polling(self: &Rc<Self>) {
        let app = self.clone();

        self.timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(100),
            move || {
                if app.weak.upgrade().is_none() {
                    return;
                }

                app.poll_playback();
                app.poll_advance();
            },
        );
    }

    pub fn register(self: &Rc<Self>) {
        self.register_polling();
        self.register_play_pause();
        self.register_next();
        self.register_prev();
    }
}
