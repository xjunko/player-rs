use image::ImageReader;
use lofty::file::TaggedFileExt;
use lofty::probe::Probe;
use walkdir::WalkDir;

pub fn format_duration_secs(secs: f64) -> String {
    let total = secs as u64;
    let mins = total / 60;
    let s = total % 60;
    format!("{}:{:02}", mins, s)
}

static ALBUM_CACHE: once_cell::sync::Lazy<
    std::sync::Mutex<
        std::collections::HashMap<String, Option<std::path::PathBuf>>,
    >,
> = once_cell::sync::Lazy::new(|| {
    std::sync::Mutex::new(std::collections::HashMap::new())
});

pub fn find_album_art_nearby(
    file_path: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let key = file_path.to_string_lossy().to_string();
    {
        let cache = ALBUM_CACHE.lock().unwrap();
        if let Some(cached) = cache.get(&key) {
            return cached.clone();
        }
    }

    let parent_dir = file_path.parent()?;
    for entry in WalkDir::new(parent_dir).max_depth(1) {
        let entry = entry.ok()?;
        if entry.file_type().is_file() {
            let file_name = entry.file_name().to_string_lossy().to_lowercase();
            if file_name.ends_with(".jpg")
                || file_name.ends_with(".jpeg")
                || file_name.ends_with(".png")
                || file_name.ends_with(".webp")
                || file_name.ends_with(".bmp")
            {
                let path = entry.path().to_path_buf();
                ALBUM_CACHE.lock().unwrap().insert(key, Some(path.clone()));
                return Some(path);
            }
        }
    }
    ALBUM_CACHE.lock().unwrap().insert(key, None);
    None
}

pub fn has_embedded_cover(path: &str) -> bool {
    if let Ok(tagged_file) = Probe::open(path).and_then(|p| p.read())
        && let Some(tag) = tagged_file.primary_tag()
    {
        return !tag.pictures().is_empty();
    }
    false
}

pub struct SmallImageData {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub fn load_small_image_embedded_data(path: &str) -> Option<SmallImageData> {
    let tagged_file = Probe::open(path).ok()?.read().ok()?;
    let tag = tagged_file.primary_tag()?;
    let pic = tag.pictures().first()?;
    let img = image::load_from_memory(pic.data()).ok()?;
    let resized = img.thumbnail(100, 100);
    let rgba = resized.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some(SmallImageData { rgba: rgba.into_raw(), width, height })
}

pub fn load_small_image_data(path: &str) -> Option<SmallImageData> {
    let img = ImageReader::open(path).ok()?.decode().ok()?;
    let resized = img.thumbnail(100, 100);
    let rgba = resized.into_rgba8();
    let (width, height) = rgba.dimensions();
    Some(SmallImageData { rgba: rgba.into_raw(), width, height })
}
