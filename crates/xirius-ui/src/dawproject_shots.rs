//! DAWproject from the song menu (EXP-006, EXP-007): the menu, writing the song with its toast
//! and the list of what it left out, a file dragged over the lanes and the song it opens with
//! what could not be read. The OS's file windows are not opened: a stage gives the answer the
//! window would.
use std::path::{Path, PathBuf};

use crate::{
    dawproject_ui::{EXPORTING, READING},
    shot::{Act, Play, Shot, Target},
    studio::{FileRequest, Geometry, Studio},
    toast::{Answer, Toast},
    topbar::BarButton,
};

const fn stage(n: u8) -> Act {
    Act::Stage(Play::Studio(prepare), n)
}

const fn press_toast(n: u8) -> Act {
    Act::Stage(Play::Laid(press), n)
}

pub const SHOTS: &[Shot] = &[
    Shot {
        name: "dawproject01-menu",
        acts: &[Act::Reset, Act::Press(Target::Top(BarButton::SongName))],
    },
    Shot {
        name: "dawproject02-writing",
        acts: &[Act::Key("escape"), stage(0)],
    },
    Shot {
        name: "dawproject03-written",
        acts: &[stage(1)],
    },
    Shot {
        name: "dawproject04-left-out",
        acts: &[press_toast(0)],
    },
    Shot {
        name: "dawproject05-drop",
        acts: &[Act::Key("escape"), stage(2)],
    },
    Shot {
        name: "dawproject06-reading",
        acts: &[stage(3)],
    },
    Shot {
        name: "dawproject07-opened",
        acts: &[stage(4)],
    },
    Shot {
        name: "dawproject08-not-read",
        acts: &[press_toast(1)],
    },
];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../target/dawproject-ui-{}", std::process::id()))
}

/// The toast while the work goes, held at one place: the work itself ends sooner than a
/// picture is taken.
fn busy(s: &mut Studio, text: String) {
    s.toast = Some(Toast {
        busy: true,
        ..Toast::plain(text)
    });
}

/// A `.dawproject` written elsewhere: a pad on a VST2 instrument, a bass, and four scenes.
fn demo_song(dir: &Path) -> PathBuf {
    let xml = r##"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Application name="Other" version="1"/>
  <Transport><Tempo unit="bpm" value="96" id="t0"/></Transport>
  <Structure>
    <Track contentType="notes" id="p" name="Pad">
      <Channel role="regular" id="pc" destination="mc">
        <Devices>
          <Vst2Plugin deviceID="1234" deviceName="Diva" deviceRole="instrument" id="d1"/>
        </Devices>
      </Channel>
    </Track>
    <Track contentType="notes" id="b" name="Bass">
      <Channel role="regular" id="bc" destination="mc"/>
    </Track>
    <Track contentType="audio notes" id="m" name="Master">
      <Channel role="master" id="mc"/>
    </Track>
  </Structure>
  <Arrangement>
    <Lanes timeUnit="beats">
      <Lanes track="p">
        <Clips>
          <Clip time="0" duration="16"><Notes>
            <Note time="0" duration="8" key="60"/><Note time="0" duration="8" key="64"/>
            <Note time="8" duration="8" key="62"/><Note time="8" duration="8" key="65"/>
          </Notes></Clip>
        </Clips>
      </Lanes>
      <Lanes track="b">
        <Clips>
          <Clip time="0" duration="16"><Notes>
            <Note time="0" duration="2" key="36"/><Note time="4" duration="2" key="36"/>
            <Note time="8" duration="2" key="38"/><Note time="12" duration="2" key="38"/>
          </Notes></Clip>
        </Clips>
      </Lanes>
    </Lanes>
  </Arrangement>
  <Scenes><Scene id="s1"/><Scene id="s2"/><Scene id="s3"/><Scene id="s4"/></Scenes>
</Project>"##;
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("Demo Song.dawproject");
    let mut zip =
        xirius_core::dawproject::zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.add("project.xml", xml.as_bytes(), true).unwrap();
    zip.finish().unwrap();
    path
}

fn prepare(s: &mut Studio, n: u8) {
    match n {
        0 => {
            // The pointer has left the song's name it pressed: no balloon.
            s.hover = crate::studio::Hit::None;
            busy(s, format!("{EXPORTING} 42%"));
        }
        1 => {
            s.toast = None;
            // The song saved with the audio its clips play, which goes inside the file.
            let song = dir().join("song");
            s.command(|it| it.save_as(&song));
            assert!(s.error.is_none(), "{:?}", s.error);
            for name in ["vocal_take1.wav", "guitar_di.wav"] {
                tone(&song.join(name));
            }
            let out = dir().join("夜の散歩");
            s.finish_file_request(FileRequest::ExportDawproject, &out);
            crate::shot::run_for(s, 60.0, |s| s.dawproject.is_none());
            let t = s.toast.as_ref().expect("the toast when it is written");
            assert!(t.done, "{}", t.text);
            let file = std::fs::File::open(out.with_extension("dawproject")).unwrap();
            let zip = xirius_core::dawproject::zip::ZipReader::new(std::io::BufReader::new(file));
            let names = zip.unwrap().names().to_vec();
            let audio = names.iter().filter(|n| n.starts_with("audio/")).count();
            assert_eq!(audio, 2, "{names:?}");
        }
        2 => {
            s.popup = None;
            s.toast = None;
            s.drop_dawproject = Some("Demo Song.dawproject".into());
        }
        3 => {
            s.drop_dawproject = None;
            busy(s, format!("{READING} 64%"));
        }
        4 => {
            s.toast = None;
            let path = demo_song(&dir());
            s.finish_file_request(FileRequest::OpenDawproject, &path);
            s.after_open(|s| {
                let t = s.toast.as_ref().expect("what could not be read");
                assert!(t.alert, "{}", t.text);
                let project = s.project();
                let names: Vec<_> = project.tracks.values().map(|t| t.name.as_str()).collect();
                assert!(
                    names.contains(&"Pad") && names.contains(&"Bass"),
                    "{names:?}"
                );
            });
        }
        _ => unreachable!(),
    }
}

/// One second of a quiet tone at `path`.
fn tone(path: &Path) {
    let x: Vec<f32> = (0..48_000)
        .map(|i| 0.1 * (i as f32 / 48_000.0 * 220.0 * std::f32::consts::TAU).sin())
        .collect();
    xirius_session::export::write_wav(path, &x, &x, 48_000, 32).unwrap();
}

/// Presses the toast's first button: the list of what was left out (`0`) or not read (`1`).
fn press(s: &mut Studio, g: &Geometry, n: u8) {
    let parts = crate::toast::layout(s, g).expect("the toast");
    let b = parts.button.expect("its button");
    assert!(s.toast_press(g, b.x + b.w / 2.0, b.y + b.h / 2.0));
    let list = s.popup.as_ref().and_then(|p| p.skipped.as_ref());
    let title = list.map(|l| l.title.as_str());
    let want = match n {
        0 => crate::dawproject_ui::LEFT_OUT,
        _ => crate::dawproject_ui::NOT_READ,
    };
    assert_eq!(title, Some(want));
    assert!(matches!(
        s.toast.as_ref().and_then(|t| t.button.as_ref()),
        Some((_, Answer::Skipped(_)))
    ));
}
