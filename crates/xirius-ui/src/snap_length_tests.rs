use crate::{
    studio::Studio,
    tools::{Mods, SnapMode, Tool},
};

#[test]
fn drawing_without_snap_uses_the_previous_grid_length_and_unsnapped_start() {
    let mut studio = Studio::sample();
    let geometry = studio.geometry(1440.0, 820.0);
    let original = studio.roll_ctx(&geometry).unwrap();
    studio.song.run("note.delete", serde_json::json!({"ids": original.notes().iter().map(|note| note.id.to_string()).collect::<Vec<_>>() })).unwrap();
    studio.set_grid_mode(SnapMode::Quantize);
    studio.song.set_grid("1/8").unwrap();
    studio.song.set_snap(false).unwrap();
    studio.tool = Tool::Draw;
    let ctx = studio.roll_ctx(&geometry).unwrap();
    let view = studio.roll.view(&ctx);
    let position = (view.grid.x + 101.0, view.grid.y + 7.0);
    let before = studio.song.steps();
    let down = studio
        .roll
        .down(&ctx, position.0, position.1, Mods::NONE, 1);
    studio.apply_roll(&geometry, down);
    assert_eq!(studio.roll.preview.ghosts[0].length, 480);
    let up = studio.roll.up(&ctx, position.0, position.1, Mods::NONE);
    studio.apply_roll(&geometry, up);
    assert!(studio.error.is_none(), "{:?}", studio.error);
    let after = studio.roll_ctx(&geometry).unwrap();
    assert_eq!(after.notes().len(), 1);
    let note = &after.notes()[0];
    assert_eq!(note.length, 480);
    assert_ne!(note.start % 480, 0);
    assert_eq!(studio.song.steps(), before + 1);
    assert!(studio.undo(&geometry));
    assert_eq!(
        studio.roll_ctx(&geometry).unwrap().notes().len(),
        ctx.notes().len()
    );
}
