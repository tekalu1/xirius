//! Lists a VST3 bundle's classes and, for audio classes, their buses, parameters and latency.
//!
//! `vst3-info [--editor DIR] [BUNDLE...]`; without bundles, every bundle under the standard
//! VST3 folder (each one is loaded, which runs its code). `--editor DIR` also opens each
//! editor without activating it, renders it into `DIR/<name>.bmp` and closes it. A resizable
//! editor is also dragged larger and given a content scale of 1.5 (`DIR/<name> resized.bmp`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use xirius_vst3::{Module, Plugin, default_vst3_dir, find_bundles};

fn main() {
    let mut args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let mut editor_dir = None;
    if args.first().is_some_and(|a| a.as_os_str() == "--editor") && args.len() >= 2 {
        editor_dir = Some(args.remove(1));
        args.remove(0);
        std::fs::create_dir_all(editor_dir.as_ref().unwrap()).expect("create the editor dir");
    }
    let bundles = if args.is_empty() {
        find_bundles(&default_vst3_dir())
    } else {
        args
    };
    for b in bundles {
        if let Err(e) = info(&b, editor_dir.as_deref()) {
            println!("{}: {e}", b.display());
        }
    }
}

fn info(bundle: &Path, editor_dir: Option<&Path>) -> Result<(), xirius_vst3::Error> {
    let module = Arc::new(Module::load(bundle)?);
    println!("{} ({})", bundle.display(), module.vendor);
    for class in module.classes() {
        println!(
            "  [{}] {} {} {}",
            class.category, class.name, class.sub_categories, class.version
        );
        if !class.is_audio_processor() {
            continue;
        }
        let mut plugin = match Plugin::new(module.clone(), &class) {
            Ok(p) => p,
            Err(e) => {
                println!("    {e}");
                continue;
            }
        };
        let params = plugin.params();
        let mut audio = plugin.audio()?;
        let latency = audio.prepare(48_000.0, 512, false)?;
        println!(
            "    in {:?} out {:?} events {} params {} latency {} controller {}",
            audio.input_channels(),
            audio.output_channels(),
            audio.has_event_input(),
            params.len(),
            latency,
            plugin.has_controller()
        );
        for p in params.iter().take(8) {
            println!(
                "    param {} {:?} default {:.3} {:?}",
                p.id,
                p.title,
                p.default,
                plugin.param_text(p.id, p.default).unwrap_or_default()
            );
        }
        if let Some(dir) = editor_dir {
            show_editor(&plugin, &dir.join(format!("{}.bmp", class.name)));
        }
    }
    Ok(())
}

fn show_editor(plugin: &Plugin, out: &Path) {
    let t = Instant::now();
    let mut editor = match plugin.open_editor(&plugin.class().name) {
        Ok(e) => e,
        Err(e) => {
            println!("    editor: {e}");
            return;
        }
    };
    let opened = t.elapsed();
    if editor.took_foreground() {
        // It should not; if it does, give the desktop back at once.
        println!("    editor: took the foreground; closed at once");
        return;
    }
    // Long enough for editors that draw on a timer.
    editor.pump(Duration::from_millis(1500));
    match editor.capture_bmp(out) {
        Ok(c) => println!(
            "    editor: {:?} opened in {:.0} ms, resizable {}, resizes {}, captured {}x{} with {} colours -> {}",
            editor.size(),
            opened.as_secs_f64() * 1e3,
            editor.can_resize(),
            editor.resize_requests(),
            c.width,
            c.height,
            c.distinct_colors,
            out.display()
        ),
        Err(e) => println!("    editor: opened, capture failed: {e}"),
    }
    println!("    editor: content scale {:?}", editor.content_scale());
    if !editor.can_resize() {
        return;
    }
    let (w, h) = editor.size();
    let dragged = editor.drag_to(w + 123, h + 77);
    editor.pump(Duration::from_millis(500));
    let scaled = editor.set_content_scale(1.5);
    editor.pump(Duration::from_millis(1000));
    let resized = out.with_file_name(format!(
        "{} resized.bmp",
        out.file_stem().unwrap_or_default().to_string_lossy()
    ));
    match editor.capture_bmp(&resized) {
        Ok(c) => println!(
            "    editor: dragged to {}x{} -> {:?}, scale 1.5 taken {scaled} -> {:?}, resizes {}, captured {}x{} with {} colours -> {}",
            w + 123,
            h + 77,
            dragged,
            editor.size(),
            editor.resize_requests(),
            c.width,
            c.height,
            c.distinct_colors,
            resized.display()
        ),
        Err(e) => println!("    editor: resized, capture failed: {e}"),
    }
}
