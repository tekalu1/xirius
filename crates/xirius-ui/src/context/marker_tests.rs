use super::tests::{BAR, choose, enabled, labels, studio};
use serde_json::json;

#[test]
fn ruler_add_and_marker_rows_run_and_undo() {
    let (mut s, g) = studio();
    let x = s.tick_x(&g, 2 * BAR) + 2.0;
    assert!(s.scene_context(&g, x, g.ruler.bottom() - 3.0));
    assert!(choose(&mut s, &g, "マーカーを追加"));
    let id = s.project().markers[0].id;
    s.song.run("marker.add", json!({"at": 4 * BAR})).unwrap();
    let r = s.marker_rect(&g, id).unwrap();
    assert!(s.scene_context(&g, r.x + 4.0, r.y + 4.0));
    assert_eq!(
        labels(&s),
        [
            "マーカー名を変更",
            "次のマーカーまでをループに",
            "色",
            "マーカーを削除"
        ]
    );
    assert!(choose(&mut s, &g, "次のマーカーまでをループに"));
    assert_eq!(s.project().loop_range.unwrap().end, 4 * BAR);
    s.undo(&g);
    assert!(s.project().loop_range.is_none());
    assert!(s.scene_context(&g, r.x + 4.0, r.y + 4.0));
    assert!(choose(&mut s, &g, "マーカーを削除"));
    assert_eq!(s.project().markers.len(), 1);
    s.undo(&g);
    assert_eq!(s.project().markers.len(), 2);
    let last = s.project().markers[1].id;
    let r = s.marker_rect(&g, last).unwrap();
    assert!(s.scene_context(&g, r.x + 4.0, r.y + 4.0));
    assert!(!enabled(&s, "次のマーカーまでをループに"));
}
