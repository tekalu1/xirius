//! The master track is stored under the English name "Master" (every song file has it), and
//! the screens show it as "マスター", as the output chooser does. The stored name stays.
use xirius_core::Project;
use xirius_core::id::TrackId;
use xirius_core::model::{Track, TrackKind};

#[test]
fn the_master_track_is_shown_as_masuta_and_stays_master_in_the_song() {
    let p = Project::new("曲");
    let master = &p.tracks[&TrackId::MASTER];
    assert_eq!(master.name, "Master", "the song file keeps its spelling");
    assert_eq!(master.shown_name(), "マスター");
}

#[test]
fn a_master_the_user_renamed_and_ordinary_tracks_show_their_own_name() {
    let p = Project::new("曲");
    let mut master = (*p.tracks[&TrackId::MASTER]).clone();
    master.name = "Main out".into();
    assert_eq!(master.shown_name(), "Main out");
    let audio = Track::new(TrackId(900), TrackKind::Audio, "Master");
    assert_eq!(
        audio.shown_name(),
        "Master",
        "only the master track is translated"
    );
}
