//! Saving PNGs and putting images on the clipboard.

use std::fs::{self, File};
use std::io::{self, BufWriter};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use image::RgbaImage;
use jiff::civil::DateTime;

/// `Kesit YYYY-MM-DD at HH.MM.SS.png`
pub fn file_name(time: DateTime) -> String {
    time.strftime("Kesit %Y-%m-%d at %H.%M.%S.png").to_string()
}

/// Creates a file named `name` in `dir` that did not exist before,
/// appending ` (2)`, ` (3)`… when two saves land in the same second.
/// Creation is exclusive, so an existing file (or a symlink planted under
/// that name) is never written through.
fn create_unique(dir: &Path, name: &str) -> Result<(PathBuf, File)> {
    let stem = name.strip_suffix(".png").unwrap_or(name);
    let candidates =
        std::iter::once(name.to_owned()).chain((2..10_000).map(|n| format!("{stem} ({n}).png")));
    for candidate in candidates {
        let path = dir.join(candidate);
        match File::create_new(&path) {
            Ok(file) => return Ok((path, file)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err).with_context(|| format!("creating {}", path.display())),
        }
    }
    bail!("no free file name for {name} in {}", dir.display())
}

/// Writes `img` as a PNG into `dir` (created if missing) and returns the path.
pub fn save_png(img: &RgbaImage, dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let now = jiff::ZonedDateTime::now().datetime();
    let (path, file) = create_unique(dir, &file_name(now))?;
    let written = img
        .write_to(&mut BufWriter::new(file), image::ImageFormat::Png)
        .with_context(|| format!("writing {}", path.display()));
    if written.is_err() {
        let _ = fs::remove_file(&path);
    }
    written.map(|()| path)
}

/// Shows `path` in the file manager: selected in Finder on macOS, its
/// folder opened with `xdg-open` on Linux. Does not wait for it.
pub fn reveal(path: &Path) -> Result<()> {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut cmd = Command::new("open");
        cmd.arg("-R").arg(path);
        cmd
    } else {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(path.parent().unwrap_or(Path::new(".")));
        cmd
    };
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("opening the file manager")?;
    reap_in_background(child);
    Ok(())
}

/// Waits for `child` on a thread, so an editor that stays open does not
/// collect zombies.
fn reap_in_background(mut child: std::process::Child) {
    std::thread::spawn(move || child.wait());
}

/// Puts `img` on the clipboard.
///
/// On macOS the pasteboard keeps the data after the process exits. On Linux
/// (X11 and Wayland) the clipboard is served by the owning process, so the
/// image is handed to a detached `kesit __clipboard-hold` process that keeps
/// serving it until another application takes the clipboard.
pub fn copy_image(img: &RgbaImage) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux_hold::spawn_holder(img)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let mut clipboard = arboard::Clipboard::new().context("opening the clipboard")?;
        clipboard
            .set_image(image_data(img))
            .context("writing the image to the clipboard")
    }
}

fn image_data(img: &RgbaImage) -> arboard::ImageData<'_> {
    arboard::ImageData {
        width: img.width() as usize,
        height: img.height() as usize,
        bytes: std::borrow::Cow::Borrowed(img.as_raw()),
    }
}

/// The image travels to the holder over a pipe (width and height as
/// little-endian `u32`, then raw RGBA), so no screenshot touches the disk.
#[cfg(any(target_os = "linux", test))]
mod pipe_format {
    use std::io::{self, Read, Write};

    use image::RgbaImage;

    /// 16384 × 16384 RGBA is 1 GiB; nothing larger is accepted.
    const MAX_SIDE: u32 = 16_384;

    pub fn write(out: &mut impl Write, img: &RgbaImage) -> io::Result<()> {
        out.write_all(&img.width().to_le_bytes())?;
        out.write_all(&img.height().to_le_bytes())?;
        out.write_all(img.as_raw())
    }

    pub fn read(input: &mut impl Read) -> io::Result<RgbaImage> {
        let mut header = [0u8; 8];
        input.read_exact(&mut header)?;
        let [w0, w1, w2, w3, h0, h1, h2, h3] = header;
        let (width, height) = (
            u32::from_le_bytes([w0, w1, w2, w3]),
            u32::from_le_bytes([h0, h1, h2, h3]),
        );
        let invalid = |msg: &str| io::Error::new(io::ErrorKind::InvalidData, msg.to_owned());
        if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
            return Err(invalid("image size out of range"));
        }
        let mut bytes = vec![0u8; width as usize * height as usize * 4];
        input.read_exact(&mut bytes)?;
        RgbaImage::from_raw(width, height, bytes).ok_or_else(|| invalid("short image data"))
    }
}

#[cfg(target_os = "linux")]
pub mod linux_hold {
    use super::*;
    use std::io::Write;

    pub fn spawn_holder(img: &RgbaImage) -> Result<()> {
        let exe = std::env::current_exe().context("locating the kesit binary")?;
        let mut child = Command::new(exe)
            .arg("__clipboard-hold")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .context("starting the clipboard holder")?;
        let sent = match child.stdin.take() {
            Some(mut stdin) => pipe_format::write(&mut stdin, img).and_then(|()| stdin.flush()),
            None => Err(io::Error::other("no pipe to the clipboard holder")),
        };
        reap_in_background(child);
        sent.context("handing the image to the clipboard holder")
    }

    /// Entry point of `kesit __clipboard-hold`: reads the image from stdin,
    /// serves it until another application takes the clipboard, then exits.
    pub fn run() -> Result<()> {
        use arboard::SetExtLinux;
        let img = pipe_format::read(&mut io::stdin().lock()).context("reading the image")?;
        let mut clipboard = arboard::Clipboard::new().context("opening the clipboard")?;
        clipboard
            .set()
            .wait()
            .image(image_data(&img))
            .context("writing the image to the clipboard")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kesit-{name}-{}", std::process::id()))
    }

    #[test]
    fn file_name_format() {
        let t = jiff::civil::date(2026, 3, 7).at(9, 5, 3, 0);
        assert_eq!(file_name(t), "Kesit 2026-03-07 at 09.05.03.png");
    }

    #[test]
    fn create_unique_never_reuses_a_name() {
        let dir = temp_dir("unique");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let name = "Kesit 2026-01-01 at 00.00.00.png";
        let (first, _) = create_unique(&dir, name).expect("first");
        assert_eq!(first, dir.join(name));
        let (second, _) = create_unique(&dir, name).expect("second");
        assert_eq!(second, dir.join("Kesit 2026-01-01 at 00.00.00 (2).png"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn create_unique_does_not_follow_symlinks() {
        let dir = temp_dir("symlink");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("dir");
        let target = dir.join("victim");
        std::os::unix::fs::symlink(&target, dir.join("shot.png")).expect("symlink");
        let (path, _) = create_unique(&dir, "shot.png").expect("create");
        assert_eq!(path, dir.join("shot (2).png"));
        assert!(!target.exists(), "wrote through the symlink");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_png_writes_file() {
        let dir = temp_dir("save");
        let img = RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255]));
        let path = save_png(&img, &dir).expect("save");
        let back = image::open(&path).expect("read back").to_rgba8();
        assert_eq!(back, img);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pipe_format_round_trips_and_rejects_bad_headers() {
        let img = RgbaImage::from_pixel(5, 3, image::Rgba([9, 8, 7, 255]));
        let mut bytes = Vec::new();
        pipe_format::write(&mut bytes, &img).expect("write");
        assert_eq!(pipe_format::read(&mut bytes.as_slice()).expect("read"), img);
        assert!(pipe_format::read(&mut &bytes[..bytes.len() - 1]).is_err());
        let huge = [u32::MAX.to_le_bytes(), 1u32.to_le_bytes()].concat();
        assert!(pipe_format::read(&mut huge.as_slice()).is_err());
    }
}
