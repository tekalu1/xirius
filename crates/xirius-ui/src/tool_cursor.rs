//! The active arrangement tool follows held modifiers at the pointer. (The pointer itself takes
//! the tool's shape in `pointer.rs`.)

#[cfg(test)]
mod tests {
    use crate::studio::Studio;
    use crate::tools::{Area, Mods, Tool};
    #[test]
    fn ctrl_reverses_the_tool_without_a_mouse_drag() {
        let mut s = Studio::sample();
        let g = s.geometry(1440.0, 820.0);
        s.arrange_move(&g, g.lanes.x + 10.0, g.lanes.y + 30.0, Mods::NONE);
        assert!(s.arrange_mods(Mods::ctrl()));
        assert_eq!(
            s.holds().tool(Area::Arrange, s.tool, s.arrange.mods),
            Tool::Range
        );
        s.tool = Tool::Draw;
        assert_eq!(
            s.holds().tool(Area::Arrange, s.tool, s.arrange.mods),
            Tool::Select
        );
        assert!(s.arrange_mods(Mods::NONE));
        assert_eq!(
            s.holds().tool(Area::Arrange, s.tool, s.arrange.mods),
            Tool::Draw
        );
    }
}
