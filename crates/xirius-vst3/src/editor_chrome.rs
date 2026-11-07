//! The frame's controls live on the same GUI thread as its viewport (UI-337).
use crate::{Caption, EditorAction, EditorBar};
use std::sync::Once;
use windows_sys::Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*};
use xirius_design::{Font, FontSize, Surface, Theme};

pub(crate) const HEIGHT: f32 = 36.0;
const BUTTON: f32 = 28.0;
pub(crate) const POWER: usize = 1;
pub(crate) const PRESET: usize = 2;
pub(crate) const DESTINATION: usize = 3;
pub(crate) const PIN: usize = 4;
pub(crate) const CLOSE: usize = 5;
pub(crate) const CHANNEL: usize = 6;
pub(crate) const SIDECHAIN: usize = 7;
const PREVIOUS: usize = 8;
const NEXT: usize = 11;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}
fn color(rgb: u32) -> u32 {
    ((rgb & 255) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 255)
}

pub(crate) struct Controls {
    pub buttons: Vec<(usize, HWND)>,
    _tips: Vec<Vec<u16>>,
}

impl Controls {
    pub fn new(hwnd: HWND) -> Self {
        let labels = [
            (POWER, "電源"),
            (PRESET, "プリセット"),
            (DESTINATION, "表示先"),
            (PIN, "ピン留め"),
            (CLOSE, "閉じる"),
            (CHANNEL, "チャンネルエディター"),
            (SIDECHAIN, "サイドチェーン"),
            (PREVIOUS, "前のプリセット"),
            (NEXT, "次のプリセット"),
        ];
        let buttons: Vec<_> = labels
            .into_iter()
            .filter_map(|(id, label)| {
                // SAFETY: child controls belong to this frame and GUI thread; text is copied.
                let button = unsafe {
                    CreateWindowExW(
                        0,
                        wide("BUTTON").as_ptr(),
                        wide(label).as_ptr(),
                        WS_CHILD
                            | WS_VISIBLE
                            | WS_TABSTOP
                            | BS_OWNERDRAW as u32
                            | if matches!(id, PREVIOUS | NEXT) {
                                WS_DISABLED
                            } else {
                                0
                            },
                        0,
                        0,
                        0,
                        0,
                        hwnd,
                        id as _,
                        std::ptr::null_mut(),
                        std::ptr::null(),
                    )
                };
                (!button.is_null()).then_some((id, button))
            })
            .collect();
        let mut tips: Vec<_> = labels.iter().map(|(_, label)| wide(label)).collect();
        // SAFETY: the tooltip is owned by this frame; retained strings outlive it.
        unsafe {
            use windows_sys::Win32::UI::Controls::*;
            let tip = CreateWindowExW(
                WS_EX_NOACTIVATE,
                TOOLTIPS_CLASSW,
                std::ptr::null(),
                WS_POPUP | TTS_ALWAYSTIP,
                0,
                0,
                0,
                0,
                hwnd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            if !tip.is_null() {
                for &(id, button) in &buttons {
                    let index = labels.iter().position(|(key, _)| *key == id).unwrap();
                    let info = TTTOOLINFOW {
                        cbSize: std::mem::size_of::<TTTOOLINFOW>() as u32,
                        uFlags: TTF_IDISHWND | TTF_SUBCLASS,
                        hwnd,
                        uId: button as usize,
                        lpszText: tips[index].as_mut_ptr(),
                        ..Default::default()
                    };
                    SendMessageW(tip, TTM_ADDTOOLW, 0, &info as *const _ as isize);
                }
            }
        }
        Self {
            buttons,
            _tips: tips,
        }
    }

    pub fn layout(&self, width: i32, scale: f32, bar: &EditorBar) {
        for &(id, hwnd) in &self.buttons {
            let r = seat(id, width as f32 / scale, bar);
            // SAFETY: these controls are children of our frame on this thread.
            unsafe {
                if let Some((x, w)) = r {
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        (x * scale).round() as i32,
                        (4.0 * scale).round() as i32,
                        (w * scale).round() as i32,
                        (BUTTON * scale).round() as i32,
                        SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                } else {
                    ShowWindow(hwnd, SW_HIDE);
                }
            }
        }
    }
}

fn seat(id: usize, width: f32, bar: &EditorBar) -> Option<(f32, f32)> {
    let right = width - 4.0;
    let compact = width < 520.0;
    let narrow = width < 380.0;
    Some(match id {
        POWER => (4.0, BUTTON),
        CLOSE => (right - BUTTON, BUTTON),
        PIN => (right - 58.0, BUTTON),
        DESTINATION => (right - 88.0, BUTTON),
        CHANNEL if !compact => (right - 118.0, BUTTON),
        SIDECHAIN if bar.sidechain => (right - if compact { 118.0 } else { 148.0 }, BUTTON),
        PRESET | PREVIOUS | NEXT => {
            let end = right
                - 90.0
                - if compact { 0.0 } else { 30.0 }
                - if bar.sidechain { 30.0 } else { 0.0 };
            let w = (width * 0.34)
                .clamp(108.0, 220.0)
                .min((end - 110.0).max(36.0));
            let step = if narrow { 0.0 } else { 24.0 };
            match id {
                PREVIOUS if !narrow => (end - w, step),
                NEXT if !narrow => (end - step, step),
                PRESET => (end - w + step, (w - 2.0 * step).max(20.0)),
                _ => return None,
            }
        }
        _ => return None,
    })
}

pub(crate) fn height(scale: f32) -> i32 {
    (HEIGHT * scale).round() as i32
}

/// The title and icons share the app's tokens. Native BUTTON children supply
/// their names and roles to accessibility, while the host draws their faces.
pub(crate) fn paint(
    hwnd: HWND,
    dc: HDC,
    bar: &EditorBar,
    caption: Option<Caption>,
    scale: f32,
    control: Option<usize>,
) {
    static FONTS: Once = Once::new();
    FONTS.call_once(|| {
        for bytes in [
            include_bytes!("../../../assets/fonts/Inter-Regular.ttf").as_slice(),
            include_bytes!("../../../assets/fonts/NotoSansJP-Regular.ttf").as_slice(),
        ] {
            let mut count = 0;
            // SAFETY: immutable embedded font bytes remain alive for the process lifetime.
            unsafe {
                AddFontMemResourceEx(
                    bytes.as_ptr() as _,
                    bytes.len() as u32,
                    std::ptr::null(),
                    &raw mut count,
                );
            }
        }
    });
    let theme = if caption.is_none_or(|c| c.dark) {
        Theme::Dark
    } else {
        Theme::Light
    };
    let pair = Surface::S1.strong_text().resolve(theme);
    let fill = color(pair.surface.to_rgb_u32());
    let ink = color(pair.ink.to_rgb_u32());
    let mut bounds = RECT::default();
    // SAFETY: our frame and its paint DC.
    unsafe {
        GetClientRect(hwnd, &mut bounds);
    }
    let width = (bounds.right - bounds.left) as f32 / scale;
    let (x, w) = control
        .and_then(|id| seat(id, width, bar))
        .unwrap_or((0.0, width));
    let r = RECT {
        left: 0,
        top: 0,
        right: (w * scale).round() as i32,
        bottom: if control.is_some() {
            (BUTTON * scale).round() as i32
        } else {
            height(scale)
        },
    };
    // SAFETY: GDI objects are selected only for this paint and released below.
    unsafe {
        let brush = CreateSolidBrush(fill);
        if control.is_none() {
            FillRect(dc, &bounds, brush);
        }
        FillRect(dc, &r, brush);
        DeleteObject(brush);
        if control == Some(PRESET) {
            let brush = CreateSolidBrush(color(
                Surface::S2
                    .strong_text()
                    .resolve(theme)
                    .surface
                    .to_rgb_u32(),
            ));
            let previous_brush = SelectObject(dc, brush);
            let previous_pen = SelectObject(dc, GetStockObject(NULL_PEN));
            let corner = (16.0 * scale).round() as i32;
            RoundRect(dc, r.left, r.top, r.right, r.bottom, corner, corner);
            SelectObject(dc, previous_brush);
            SelectObject(dc, previous_pen);
            DeleteObject(brush);
        }
        SetBkMode(dc, TRANSPARENT as i32);
        SetTextColor(dc, ink);
        let label = match control {
            Some(PRESET) => {
                if bar.preset.is_empty() {
                    "—"
                } else {
                    &bar.preset
                }
            }
            Some(_) => "",
            None => "",
        };
        let family = if label.is_ascii() && bar.track.is_ascii() && bar.device.is_ascii() {
            Font::Inter
        } else {
            Font::NotoSansJp
        };
        let font = CreateFontW(
            -(FontSize::Body.px() * scale).round() as i32,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET as u32,
            OUT_DEFAULT_PRECIS as u32,
            CLIP_DEFAULT_PRECIS as u32,
            CLEARTYPE_QUALITY as u32,
            DEFAULT_PITCH as u32,
            wide(family.family()).as_ptr(),
        );
        let old_font = SelectObject(dc, font);
        if control == Some(PRESET) {
            let mut text_r = r;
            let chevron = width >= 380.0;
            text_r.left += ((if chevron { 24.0 } else { 10.0 }) * scale).round() as i32;
            text_r.right -= (6.0 * scale).round() as i32;
            if chevron {
                let face = color(
                    Surface::S2
                        .strong_text()
                        .resolve(theme)
                        .surface
                        .to_rgb_u32(),
                );
                crate::editor_icons::paint(
                    dc,
                    12,
                    8.0 * scale,
                    9.0 * scale,
                    scale * 10.0 / 16.0,
                    ink,
                    face,
                );
            }
            let text = wide(label);
            DrawTextW(
                dc,
                text.as_ptr(),
                -1,
                &mut text_r,
                DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
        } else if control.is_none() {
            let preset_x = seat(PREVIOUS, width, bar)
                .or_else(|| seat(PRESET, width, bar))
                .map_or(width - 94.0, |v| v.0);
            let name = if width < 520.0 {
                bar.device.clone()
            } else {
                format!("{} › {}", bar.track, bar.device)
            };
            let mut text_r = RECT {
                left: (58.0 * scale).round() as i32,
                top: 0,
                right: ((preset_x - 6.0) * scale).round() as i32,
                bottom: height(scale),
            };
            DrawTextW(
                dc,
                wide(&name).as_ptr(),
                -1,
                &mut text_r,
                DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
            );
            crate::editor_icons::paint(
                dc,
                if bar.instrument { 9 } else { 10 },
                36.0 * scale,
                10.0 * scale,
                scale,
                ink,
                fill,
            );
        } else if let Some(id) = control {
            let lit = (id == POWER && bar.powered) || (id == PIN && bar.pinned);
            let ink = if matches!(id, PREVIOUS | NEXT) || (id == POWER && !bar.powered) {
                color(
                    xirius_design::StructureLine::color(xirius_design::LineState::Rest, theme)
                        .to_rgb_u32(),
                )
            } else if lit {
                color(
                    xirius_design::LevelMeter::default().gradient(theme).stops()[0]
                        .color
                        .to_rgb_u32(),
                )
            } else {
                ink
            };
            crate::editor_icons::paint(dc, id, 6.0 * scale, 6.0 * scale, scale, ink, fill);
        }
        SelectObject(dc, old_font);
        DeleteObject(font);
    }
    let _ = x;
}

pub(crate) fn command(hwnd: HWND, id: usize) -> Option<EditorAction> {
    match id {
        POWER => Some(EditorAction::Power),
        PIN => Some(EditorAction::Pin),
        CHANNEL => Some(EditorAction::Channel),
        SIDECHAIN => Some(EditorAction::Sidechain),
        CLOSE => {
            unsafe {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
            None
        }
        PRESET | DESTINATION => {
            // SAFETY: a local native popup; it only returns an action for asynchronous delivery.
            unsafe {
                let menu = CreatePopupMenu();
                let labels: &[(usize, &str)] = if id == PRESET {
                    &[(1, "プリセットを読み込む…"), (2, "プリセットを保存…")]
                } else {
                    &[(3, "独立ウィンドウ"), (4, "チャンネルエディター")]
                };
                for &(key, label) in labels {
                    AppendMenuW(
                        menu,
                        MF_STRING | if key == 3 { MF_CHECKED } else { 0 },
                        key,
                        wide(label).as_ptr(),
                    );
                }
                let mut at = RECT::default();
                GetWindowRect(GetDlgItem(hwnd, id as i32), &mut at);
                let chosen = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_NONOTIFY,
                    at.left,
                    at.bottom,
                    0,
                    hwnd,
                    std::ptr::null(),
                );
                DestroyMenu(menu);
                match chosen {
                    1 => Some(EditorAction::PresetOpen),
                    2 => Some(EditorAction::PresetSave),
                    4 => Some(EditorAction::Channel),
                    _ => None,
                }
            }
        }
        _ => None,
    }
}
