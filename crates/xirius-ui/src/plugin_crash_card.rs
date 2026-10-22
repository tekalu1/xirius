use crate::{
    health::{PlugAct, PluginView},
    popup::{Action, Cell, Look},
    studio::Rect,
};
use xirius_design::{Layout, Space};

pub(crate) fn cells(view: &PluginView, width: f32) -> Vec<Cell> {
    let pad = Layout::POPUP_PAD;
    let inner = width - pad * 2.0;
    let y = pad + Layout::MENU_ROW_H + Space::X1.px();
    let half = (inner - Space::X1.px()) / 2.0;
    vec![
        Cell::new(Rect::new(pad, pad, inner, Layout::MENU_ROW_H), Look::Down).text(&view.name),
        Cell::new(Rect::new(pad, y, half, 30.0), Look::Primary)
            .text("再起動")
            .icon("restore")
            .act(Action::Plugin(PlugAct::Restart)),
        Cell::new(
            Rect::new(pad + half + Space::X1.px(), y, half, 30.0),
            Look::Button,
        )
        .text("外す")
        .icon("close")
        .act(Action::Plugin(PlugAct::Remove)),
    ]
}
