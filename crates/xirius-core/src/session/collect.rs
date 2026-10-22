//! Saving gathers the audio files a song uses into the song's folder (BRW-003, BRW-004), so the
//! folder alone opens anywhere.
//!
//! - A file outside the folder is copied to the media folder's `Imports` (`Media/Imports`) and
//!   the clips that name it are pointed at the copy by one history step (`clip.relink`), so it
//!   can be undone. A song whose `copy_imports` is off keeps such files where they are.
//! - A file in the media folder is the song's even when that folder is outside the song's.
//! - A file named relative to the folder the song is saved from (a recording) is copied to the
//!   same place under the folder it is saved to; "save as" would otherwise leave it behind.
//! - A file that cannot be found stays as it is, and is reported.
//!
//! The history keeps the older states as they were: a state that names a file by its full path
//! still does, and the copy only serves the song as it is now.

use super::*;
use crate::model::Media;

/// Is the file a clip names (`name`) outside both the song's folder `song` and its media
/// folder? A relative name is the song's.
pub fn is_external(media: &Media, song: &Path, name: &str) -> bool {
    let p = Path::new(name.trim());
    if !p.is_absolute() {
        return false;
    }
    let real = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let file = real(p);
    !file.starts_with(real(song)) && !file.starts_with(real(&media.root(song)))
}

/// Copies `src` into `folder` under its own name, or `name 2.ext`, `name 3.ext`... when
/// another file holds it (a file with the same bytes is reused, and none of `taken` is). Gives
/// the copy's path.
pub fn copy_into(folder: &Path, src: &Path, taken: &[PathBuf]) -> CmdResult<PathBuf> {
    let file = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "audio".into());
    let dest = free_place(folder, &file, src, taken);
    fs::create_dir_all(folder)?;
    if !same_file(src, &dest) {
        saving::copy_file(src, &dest)?;
    }
    Ok(dest)
}

/// `path` as a clip names it in a song saved in `song`: relative with `/` inside, else full.
pub fn song_name(path: &Path, song: &Path) -> String {
    let root = fs::canonicalize(song).unwrap_or_else(|_| song.to_path_buf());
    let real = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match real.strip_prefix(&root) {
        Ok(rel) => rel_text(rel),
        Err(_) => path.display().to_string(),
    }
}

/// What a save did to the song's files.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Collected {
    /// Files copied into the folder.
    pub copied: Vec<String>,
    /// Clips now pointing at a copy.
    pub relinked: usize,
    /// Files the clips name that were not found.
    pub missing: Vec<String>,
}

impl Collected {
    /// A sentence for the save's result; empty when nothing was gathered or missing.
    pub fn say(&self) -> String {
        let mut out = String::new();
        if !self.copied.is_empty() {
            out.push_str(&format!(
                "。音声ファイル {} 個を曲のフォルダーに集めました",
                self.copied.len()
            ));
        }
        if !self.missing.is_empty() {
            out.push_str(&format!(
                "。見つからない音声ファイル {} 個はそのままです（{}）",
                self.missing.len(),
                self.missing.join("、")
            ));
        }
        out
    }
}

impl Session {
    /// [`Session::save`], gathering the files first.
    pub fn save_collecting(&mut self) -> CmdResult<Collected> {
        let dir = self.store.as_ref().ok_or_else(no_store)?.dir.clone();
        let got = self.collect_files(Some(&dir), &dir)?;
        self.save()?;
        Ok(got)
    }

    /// The AI's save, gathering the files first (AI-018). The song file is checked before any
    /// file is gathered, so a song changed elsewhere is left alone whole.
    pub(super) fn save_over_collecting(&mut self) -> CmdResult<(Collected, Overwrite)> {
        let store = self.store.clone().ok_or_else(no_store)?;
        store.check_project(&self.expected_on_disk()?)?;
        let got = self.collect_files(Some(&store.dir), &store.dir)?;
        let done = self.save_over()?;
        Ok((got, done))
    }

    /// [`Session::save_as`], gathering the files first.
    pub fn save_as_collecting(&mut self, dir: impl AsRef<Path>) -> CmdResult<Collected> {
        let dir = dir.as_ref();
        let from = self.store.as_ref().map(|s| s.dir.clone());
        // The new folder has to be empty when the song moves in, so the files follow it.
        self.save_as(dir)?;
        let got = self.collect_files(from.as_deref(), dir)?;
        if got.relinked > 0 || !got.copied.is_empty() {
            self.save()?;
        }
        Ok(got)
    }

    /// Copies what the song's audio clips name into `to` and points the clips at the copies.
    /// `from` is the folder their relative paths are taken from.
    fn collect_files(&mut self, from: Option<&Path>, to: &Path) -> CmdResult<Collected> {
        let mut got = Collected::default();
        let root = fs::canonicalize(to)?;
        let mut names: Vec<String> = Vec::new();
        for t in self.current.tracks.values() {
            for c in &t.clips {
                if let crate::model::ClipBody::Audio { file, .. } = &c.body
                    && !names.contains(file)
                {
                    names.push(file.clone());
                }
            }
            if let Some(z) = &t.frozen
                && !names.contains(&z.file)
            {
                names.push(z.file.clone());
            }
        }
        let media = self.current.media.clone();
        let imports = media.place(&root, Media::IMPORTS);
        let mut pairs: Vec<Value> = Vec::new();
        let mut taken: Vec<PathBuf> = Vec::new();
        for name in names {
            let text = name.trim();
            let absolute = Path::new(text).is_absolute();
            let src = if absolute {
                Path::new(text).to_path_buf()
            } else if let Some(from) = from {
                from.join(text)
            } else {
                got.missing.push(name.clone());
                continue;
            };
            let Ok(real) = fs::canonicalize(&src) else {
                got.missing.push(name.clone());
                continue;
            };
            if let Ok(rel) = real.strip_prefix(&root) {
                // Already in the folder: say so by a path inside it.
                if absolute {
                    let rel = rel_text(rel);
                    pairs.push(json!({ "from": name, "to": rel }));
                }
                continue;
            }
            if absolute {
                // A file of the media folder outside the song's is the song's; another is
                // copied in only when the song says so.
                if !is_external(&media, &root, text) || !media.copy_imports {
                    continue;
                }
                let dest = copy_into(&imports, &real, &taken)?;
                taken.push(dest.clone());
                let shown = song_name(&dest, &root);
                got.copied.push(file_name(&dest));
                pairs.push(json!({ "from": name, "to": shown }));
                continue;
            }
            let rel = text.replace('\\', "/");
            let dest = root.join(&rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            if !same_file(&real, &dest) {
                saving::copy_file(&real, &dest)?;
            }
            got.copied
                .push(rel.rsplit('/').next().unwrap_or(&rel).to_string());
        }
        if !pairs.is_empty() {
            let r = self.run_value(
                "clip.relink",
                json!({ "files": pairs }),
                &RunOptions::default(),
            )?;
            got.relinked = r.data["relinked"].as_u64().unwrap_or(0) as usize;
        }
        Ok(got)
    }
}

/// `p` with `/`, however the system writes it.
fn rel_text(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Both files exist, are as long as each other, and hold the same bytes.
fn same_file(a: &Path, b: &Path) -> bool {
    let (Ok(x), Ok(y)) = (fs::metadata(a), fs::metadata(b)) else {
        return false;
    };
    x.len() == y.len() && fs::read(a).ok() == fs::read(b).ok()
}

/// The last part of `p`.
fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A place for `file` in `folder`: its own name, or `name 2.ext`, `name 3.ext`... when another
/// file holds it (a file with the same bytes is reused).
fn free_place(folder: &Path, file: &str, src: &Path, taken: &[PathBuf]) -> PathBuf {
    let (stem, ext) = match file.rfind('.') {
        Some(i) if i > 0 => (&file[..i], &file[i..]),
        _ => (file, ""),
    };
    for n in 1.. {
        let name = if n == 1 {
            file.to_string()
        } else {
            format!("{stem} {n}{ext}")
        };
        let path = folder.join(&name);
        let listed = taken.iter().any(|t| t == &path);
        if !listed && (!path.exists() || same_file(src, &path)) {
            return path;
        }
    }
    unreachable!()
}
