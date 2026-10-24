// 生成した file。直さない。元は tokens.json で、作り直すのは
// `cargo run -p xirius-design-lint -- tokens --write`（元と合わなければ検査が落ちる）。

/// `palette!` に渡す、全トークンの色（ダーク・ライト）。
macro_rules! palette_tokens {
    ($m:ident) => {
        $m! {
            // ---- 面（surface）。青みのある無彩色に近い階調。純黒は使わない ----
            surface_paper: c(0x1b1c23), c(0xffffff);
            surface_0: c(0x121318), c(0xeceef4);
            surface_1: c(0x222329), c(0xf4f5f9);
            surface_2: c(0x15161c), c(0xe8eaf1);
            surface_3: c(0x2a2b32), c(0xdcdee8);
            surface_hover: c(0x222329), c(0xf4f5f9);
            surface_sel: c(0x2a2b32), c(0xe3e5ed);
            /// クリップの面。中立。トラックの色は面に塗らず、中身（ノート・波形）にだけ使う。
            surface_clip: c(0x35373f), c(0xd5d8e4);
            /// 選んだクリップの面。枠を引かず、クリップの面との明るさの差で示す（ダークは明るく、ライトは暗く）。トラックの色の中身が 3:1 を保てる幅に留める。
            surface_clip_sel: c(0x41434c), c(0xc9cdda);
            /// 選んだトラックの面（頭とミキサーのストリップ、1 本でも複数でも同じ）。枠を引かず、面の明るさの差で示す。フェーダーのつまみの面とは別の色（UI-340）。
            surface_track_sel: c(0x41434c), c(0xc9cdda);
            /// ミキサーのストリップを 1 秒ほど光らせる面（頭の空いた所のダブルクリック）。そのときストリップは選んであるので、選んだトラックの面から L* で 5 以上離す（UI-340）。
            surface_strip_flash: c(0x50535e), c(0xb3b8c8);
            surface_thumb: c(0x35373f), c(0xc9cbd2);
            surface_thumb_hover: c(0x4a4c54), c(0xacaeb7);
            /// 設定のオフのトグルの溝。周囲の中立の面、白いつまみの両方と 3:1 以上（UI-317）。
            surface_switch_off: c(0x80828e), c(0x707481);
            surface_switch_off_hover: c(0x8b8d99), c(0x616572);
            surface_veil: ca(0x000000, 140), ca(0x15161c, 82);
            // ---- 塗り（fill）。有彩色の面は主要操作のボタンだけ ----
            fill_primary: c(0x5665cd), c(0x3a499e);
            fill_primary_hover: c(0x4f5ec8), c(0x2f3c86);
            /// 閉じる・録音準備の塗り（危険・取り消せない操作）。上の文字は `ink_on_fill`。
            fill_danger: c(0xc42b1c), c(0xc42b1c);
            fill_danger_hover: c(0xa9241a), c(0xa9241a);
            /// ミュート・ソロが入っているときの小さな札の塗り（札の上の文字は `ink_toggle_mute` など）。
            fill_toggle_mute: c(0xe3c14b), c(0xe3c14b);
            ink_toggle_mute: c(0x201a00), c(0x201a00);
            fill_toggle_solo: c(0x4cc38a), c(0x4cc38a);
            ink_toggle_solo: c(0x06210f), c(0x06210f);
            // ---- 文字（ink）。`ink-weak` に当たるものは作らない ----
            ink_strong: c(0xf4f6ff), c(0x0f1330);
            ink_body: c(0xdfe3f2), c(0x1c2247);
            ink_blue: c(0x9eabde), c(0x3a499e);
            ink_mark: c(0xe07ab8), c(0xaa2678);
            ink_alert: c(0xff7a70), c(0xa92a22);
            ink_ai: c(0xb48cff), c(0x6a3cc8);
            ink_on_fill: c(0xf4f5ff), c(0xf4f5ff);
            /// トラックの色で塗った面（アレンジのセクション）の上の文字。
            ink_on_track: c(0x0d0f16), c(0xffffff);
            // ---- 文字ではない色（図形・フォーカスの輪）。3:1 以上 ----
            line_focus: c(0x728cd7), c(0x5665cd);
            line_ai: c(0xb48cff), c(0x7d4fe0);
            /// 書き出しの後に曲が変わった印の点。
            alert_stale_dot: c(0xdcb36a), c(0x9a6a00);
            meter_ok: c(0x4cc38a), c(0x1f8a56);
            meter_hot: c(0xe3c14b), c(0x947500);
            meter_over: c(0xef5a50), c(0xd9443a);
            meter_well: c(0x0c0d11), c(0xd7dae4);
            // ---- ピアノロールの鍵盤 ----
            surface_key_white: c(0xffffff), c(0xffffff);
            surface_key_black: c(0x3a3e52), c(0x3a3e52);
            ink_on_key: c(0x0d0f16), c(0x0d0f16);
            /// 押している・触れている鍵（フォーカスの輪の色）の上の音名。
            ink_on_key_lit: c(0x0d0f16), c(0xffffff);
            // ---- 構造線（フォルダーの範囲の線。`StructureLine` だけが取り出せる）。中立の面の上で 3:1 以上 ----
            line_structure: c(0x8a91b8), c(0x6b7190);
            line_structure_hover: c(0x728cd7), c(0x5665cd);
            line_structure_selected: c(0x9eabde), c(0x3a499e);
            // ---- 時間のグリッドの線（小節・拍・分割・細かい）。`GridLine` だけが取り出せる。
            // 文字の色を薄く重ねる。どの面の上でも 小節 > 拍 > 分割 > 細かい の比（UI-339） ----
            line_grid_bar: ca(0xdfe3f2, 77), ca(0x1c2247, 102);
            line_grid_beat: ca(0xdfe3f2, 51), ca(0x1c2247, 66);
            line_grid_sub: ca(0xdfe3f2, 29), ca(0x1c2247, 37);
            line_grid_fine: ca(0xdfe3f2, 15), ca(0x1c2247, 19);
            // ---- 浮く面の影 ----
            surface_shadow: ca(0x000000, 128), ca(0x3a499e, 41);
            surface_shadow_near: ca(0x000000, 77), ca(0x3a499e, 26);
            // ---- 操作の分類の 5 色（設定のキーボードの点と凡例。`Category` だけが取り出せる）。
            // トラックの色とは別の族。中立の面の上で 3:1 以上 ----
            line_category_tool: c(0x8fb4ff), c(0x3459c4);
            line_category_play: c(0x6fd6a8), c(0x1b8a5c);
            line_category_edit: c(0xf2c46d), c(0x9a6b00);
            line_category_track: c(0xef8fc8), c(0xb0347f);
            line_category_view: c(0xc7a4ff), c(0x7048c8);
            // ---- トラックの 16 色（識別色）。`TrackColor` の添字順 ----
            track_0: c(0x5b8def), c(0x3e6dce);
            track_1: c(0x3fb6a8), c(0x1a7f74);
            track_2: c(0x7cc46a), c(0x417f34);
            track_3: c(0xd6b24a), c(0x8b6d17);
            track_4: c(0xe08a4f), c(0xae5b25);
            track_5: c(0xd96a9a), c(0xbb497d);
            track_6: c(0x9a7fe0), c(0x7a5cc9);
            track_7: c(0x8c95ad), c(0x68718a);
            track_8: c(0xc9a27a), c(0x8d6b48);
            track_9: c(0x5fb3d9), c(0x2779a0);
            track_10: c(0xf2897c), c(0xa8372e);
            track_11: c(0x9db854), c(0x566f00);
            track_12: c(0x4ec491), c(0x007c47);
            track_13: c(0x00c0da), c(0x007694);
            track_14: c(0x98a2fc), c(0x5255b6);
            track_15: c(0xdd8acb), c(0x943b84);
        }
    };
}

impl crate::Space {
    pub const fn px(self) -> f32 {
        match self {
            crate::Space::X1 => 4.0,
            crate::Space::X2 => 8.0,
            crate::Space::X3 => 12.0,
            crate::Space::X4 => 16.0,
            crate::Space::X5 => 24.0,
            crate::Space::X6 => 32.0,
        }
    }
}

impl crate::Radius {
    pub const fn px(self) -> f32 {
        match self {
            crate::Radius::Clip => 3.0,
            crate::Radius::L => 20.0,
            crate::Radius::M => 14.0,
            crate::Radius::Pill => 999.0,
            crate::Radius::S => 8.0,
            crate::Radius::Xs => 2.0,
        }
    }
}

impl crate::FontSize {
    pub const fn px(self) -> f32 {
        match self {
            crate::FontSize::Body => 14.0,
            crate::FontSize::Caption => 11.0,
            crate::FontSize::Lead => 15.0,
            crate::FontSize::Micro => 10.0,
            crate::FontSize::Readout => 30.0,
            crate::FontSize::Small => 12.0,
            crate::FontSize::Title => 18.0,
        }
    }
}

impl crate::LineHeight {
    pub const fn ratio(self) -> f32 {
        match self {
            crate::LineHeight::Body => 1.6,
            crate::LineHeight::Dense => 1.2,
        }
    }
}

impl crate::Weight {
    pub const fn value(self) -> u16 {
        match self {
            crate::Weight::Medium => 500,
            crate::Weight::Regular => 400,
            crate::Weight::Semibold => 600,
        }
    }
}

impl crate::Duration {
    pub const fn ms(self) -> u32 {
        match self {
            crate::Duration::Base => 240,
            crate::Duration::Fast => 120,
            crate::Duration::Slow => 560,
        }
    }
}

impl crate::Font {
    /// OS に登録するときの family 名。
    pub const fn family(self) -> &'static str {
        match self {
            crate::Font::Inter => "Inter",
            crate::Font::NotoSansJp => "Noto Sans JP",
        }
    }
}

/// AI が動いている印の 1 周（ミリ秒）。
pub const AI_LOOP_MS: u32 = 1000;

impl crate::Easing {
    /// 部品の出入り。
    pub const OUT: crate::Easing = crate::Easing([0.2, 0.7, 0.2, 1.0]);
    /// 大きな面。ゆっくり始まり、ゆっくり止まる。
    pub const ORGANIC: crate::Easing = crate::Easing([0.65, 0.0, 0.25, 1.0]);
}

impl crate::Layout {
    // ---- 一体のトップバー（タイトルバーを兼ねる） ----
    pub const TOP_BAR_H: f32 = 40.0;
    /// 引き出しの下のバー（トランスポートと CPU、UI-300）。
    pub const BOTTOM_BAR_H: f32 = 40.0;
    /// Windows の最小化・最大化・閉じるのボタンの幅。
    pub const CAPTION_BUTTON_W: f32 = 46.0;
    /// macOS: 信号機のボタンの位置（左・上）と、左に空ける幅。
    pub const TRAFFIC_LIGHT_X: f32 = 16.0;
    pub const TRAFFIC_LIGHT_Y: f32 = 14.0;
    pub const TRAFFIC_LIGHT_ROOM: f32 = 76.0;
    /// トップバーの小さいボタン（アイコン）。
    pub const ICON_BUTTON: f32 = 28.0;
    pub const ICON_SIZE: f32 = 16.0;
    pub const LOGO_H: f32 = 18.0;
    // ---- 3 つの領域 ----
    /// 領域どうしの間（面の色の差で分かれるので、間は空き）。
    pub const REGION_GAP: f32 = 6.0;
    /// 下の引き出しの高さ（ピアノロール・ミキサー）。
    pub const DRAWER_H: f32 = 280.0;
    pub const DRAWER_BAR_H: f32 = 32.0;
    // ---- トラック列とアレンジ ----
    pub const RULER_H: f32 = 28.0;
    /// ルーラーの下の、セクション（イントロ・ヴァース…）の帯の高さ。
    pub const SECTION_BAND_H: f32 = 24.0;
    /// トラックの頭の幅（[`Layout::HEAD_ROOM_DEPTH`] 段までの入れ子が収まる）。
    pub const TRACK_HEAD_W: f32 = 216.0;
    pub const TRACK_ROW_H: f32 = 64.0;
    pub const FOLDER_ROW_H: f32 = 40.0;
    /// 塊（フォルダー・続くトラックのまとまり）どうしの間。
    pub const BLOCK_GAP: f32 = 4.0;
    /// 入れ子 1 段ごとの字下げ。
    pub const INDENT: f32 = 16.0;
    /// 行の中のアイコンの大きさ。
    pub const ROW_ICON: f32 = 24.0;
    /// ミュート・ソロなどの札。
    pub const TOGGLE: f32 = 20.0;
    pub const TOGGLE_GAP: f32 = 3.0;
    /// 1 拍あたりの幅（アレンジ）。
    pub const BEAT_W: f32 = 16.0;
    pub const PLAYHEAD_W: f32 = 2.0;
    // ---- 左のブラウザー（レールとパネル） ----
    pub const RAIL_W: f32 = 40.0;
    pub const BROWSER_W: f32 = 216.0;
    pub const BROWSER_ROW_H: f32 = 28.0;
    pub const BROWSER_GROUP_H: f32 = 26.0;
    // ---- ルーラーの下の行（アレンジ・テンポ）と、トラックの頭の広がり ----
    pub const TEMPO_LANE_H: f32 = 40.0;
    /// 広げたトラックの高さ（出力の選びが入る）。
    pub const TRACK_ROW_TALL_H: f32 = 96.0;
    /// 入れ子の字下げは、この深さまで。それより深いものは点で示す。
    pub const INDENT_MAX_DEPTH: usize = 3;
    /// [`Layout::TRACK_HEAD_W`] のままで頭の部品が収まる深さ。これより深く字下げする曲では、頭を 1 段ごとに [`Layout::INDENT`] だけ広げる（[`Layout::track_head_w`]）。
    pub const HEAD_ROOM_DEPTH: usize = 2;
    /// 深さの点は、頭の左端に縦に並べ、行の高さの中央に置く（4 つまで）。
    pub const DEPTH_DOT: f32 = 2.0;
    pub const DEPTH_DOT_GAP: f32 = 3.0;
    pub const DEPTH_DOT_X: f32 = 3.0;
    pub const DEPTH_DOTS_MAX: usize = 4;
    // ---- ポップアップ（履歴・グリッド・曲のメニュー・版の一覧） ----
    pub const MENU_ROW_H: f32 = 28.0;
    pub const POPUP_PAD: f32 = 6.0;
    pub const MENU_W: f32 = 232.0;
    // ---- ピアノロール ----
    pub const PIANO_KEYS_W: f32 = 56.0;
    pub const NOTE_ROW_H: f32 = 10.0;
    // ---- ミキサー・メーター ----
    pub const METER_STRIP_H: f32 = 96.0;
    pub const METER_GAP: f32 = 3.0;
    pub const METER_HOLD_H: f32 = 2.0;
    // ---- 計測の窓（`--bench`）の列の幅など ----
    pub const BENCH_HEADER_W: f32 = 168.0;
    pub const BENCH_TRACK_H: f32 = 44.0;
    pub const BENCH_TRACK_GAP: f32 = 2.0;
}
