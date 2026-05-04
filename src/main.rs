use std::error::Error;
use std::rc::Rc;

use clap::Parser;

mod app;
use app::Application;

mod utils;

slint::include_modules!();

#[derive(clap::Parser)]
struct Args {
    /// The root directory to search for music files
    path: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let window = AppWindow::new()?;
    let stream = rodio::DeviceSinkBuilder::open_default_sink()?;
    let sink = Rc::new(rodio::Player::connect_new(stream.mixer()));
    {
        sink.pause();
        sink.set_volume(0.1);
    }

    let args = Args::parse();
    let app = Application::build(&window, sink, args)?;

    app.register();
    window.run()?;

    Ok(())
}
