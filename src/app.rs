use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use slint::{ComponentHandle, SharedString};
use walkdir::WalkDir;

use crate::{AppWindow, Args, State, Track, utils};

struct CoverUpdate {
    track_path: String,
    cover_data: Option<utils::SmallImageData>,
}

pub struct Application {
    weak: slint::Weak<AppWindow>,
    sink: Rc<rodio::Player>,
    timer: slint::Timer,

    track_i: RefCell<usize>,
    current_track_path: RefCell<String>,
    tracks: Vec<Track>,

    cover_tx: mpsc::Sender<CoverUpdate>,
    cover_rx: RefCell<mpsc::Receiver<CoverUpdate>>,
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
                // this is a bit scuffed, but basically
                // 1. we check if theres any embedded image, if there is, we set the cover_path to something that isnt empty.
                // 2. if not, we look for nearby album art, and if we find it, we set the cover_path to that.
                let mut cover_path = String::new();
                let parent_folder = entry.path().parent().unwrap();

                if utils::has_embedded_cover(
                    entry.path().to_str().unwrap_or(""),
                ) {
                    cover_path = "embedded".to_string();
                } else {
                    if let Some(album_art_path) =
                        utils::find_album_art_nearby(parent_folder)
                    {
                        cover_path =
                            album_art_path.to_string_lossy().into_owned();
                    }
                }

                if !cover_path.is_empty() {
                    println!(
                        "Found cover for {}: {}",
                        entry.path().display(),
                        cover_path
                    );
                }

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
                    cover: slint::Image::default(),
                    cover_path: cover_path.into(),
                });
            }
        }
        println!("Found {} tracks", tracks.len());

        let (cover_tx, cover_rx) = mpsc::channel();
        Ok(Rc::new(Self {
            weak: window.as_weak(),
            sink,
            timer: slint::Timer::default(),
            track_i: RefCell::new(0),
            current_track_path: RefCell::new(String::new()),
            tracks,
            cover_tx,
            cover_rx: RefCell::new(cover_rx),
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

    fn poll_cover_updates(&self) {
        let receiver = self.cover_rx.borrow();

        while let Ok(update) = receiver.try_recv() {
            if *self.current_track_path.borrow() != update.track_path {
                continue;
            }

            let Some(cover_data) = update.cover_data else {
                continue;
            };

            let buffer =
                slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                    &cover_data.rgba,
                    cover_data.width,
                    cover_data.height,
                );
            let mut track = self.window().global::<State>().get_track();
            track.cover = slint::Image::from_rgba8(buffer);
            self.window().global::<State>().set_track(track);
        }
    }

    fn set_playback_to(&self, track: &Track) {
        let track_path = track.path.to_string();
        let cover_path = track.cover_path.to_string();

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

        *self.current_track_path.borrow_mut() = track_path.clone();
        {
            let window = self.window();
            window.global::<State>().set_track(track.clone());
        }

        if !cover_path.is_empty() {
            let sender = self.cover_tx.clone();
            std::thread::spawn(move || {
                let cover_data = if cover_path == "embedded" {
                    utils::load_small_image_embedded_data(&track_path)
                } else {
                    utils::load_small_image_data(&cover_path)
                };

                let _ = sender.send(CoverUpdate { track_path, cover_data });
            });
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
                app.poll_cover_updates();
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
