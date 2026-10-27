//! Xirius UI: GPUI views and the performance spikes UI-023 / UI-024.

pub mod a11y;
pub mod about;
pub mod about_crates_shots;
pub mod about_shots;
pub mod adjust_report;
pub mod ai;
pub mod aiarc_shots;
mod ancillary_io;
pub mod app;
pub mod app_menu;
pub mod arrange;
pub mod arrange_v1_shots;
pub mod assets;
pub mod audio_job;
pub mod audio_settings;
pub mod audioproc_shots;
pub mod audioset_shots;
pub mod audition;
pub mod auto_lock;
pub mod auto_mark;
pub mod automation_modes_shots;
pub mod automation_pdc_shots;
pub mod automation_point_edit;
pub mod automation_point_edit_shots;
pub mod automation_preferences_shots;
pub mod automation_send_shots;
pub mod automation_targets;
pub mod automation_targets_shots;
pub mod automation_track_mode;
pub mod automation_track_mode_shots;
pub mod autovalue;
pub mod bar_tempo;
pub mod bar_tools;
pub mod bar_tools_shots;
pub mod bench;
pub mod bounce_row;
pub mod browser;
pub mod browser_builtin_shots;
pub mod browser_files;
pub mod browser_files_menu_shots;
mod browser_import_io;
pub mod browser_inst;
#[cfg(test)]
mod browser_io_tests;
mod browser_menu_io;
pub mod browser_pick;
pub mod browser_pool;
mod browser_prefs_io;
mod browser_prefs_save;
#[cfg(test)]
mod browser_review_tests;
pub mod bugs2_shots;
pub mod caption;
pub mod caption_shots;
pub mod channel_step_shots;
pub mod chrome;
pub mod cleanup_dialog;
pub mod clip_marks;
pub mod clip_menu_rows_shots;
pub mod clipedit_shots;
pub mod clipops;
pub mod command_notice;
pub mod context;
mod controls_shots;
pub mod cpu_list;
pub mod cpu_ping_shots;
pub mod crash_entry_shots;
pub mod crash_reports;
pub mod ctx2_shots;
pub mod ctx_shots;
pub mod ctxfolder_shots;
pub mod ctxnames_shots;
pub mod ctxshape_shots;
pub mod data;
pub mod dawproject_reference_shots;
pub mod dawproject_shots;
pub mod dawproject_ui;
pub mod dblclick_shots;
pub mod disabled_reason;
pub mod drag_scroll;
pub mod draw;
pub mod draw_length_shots;
pub mod drawer_edge;
pub mod drawer_shots;
pub mod drive;
pub mod drop_audio;
pub mod dropdown;
pub mod drum_names_shots;
pub mod drum_rows_shots;
pub mod drum_view_shots;
pub mod e2e_shots;
pub mod edit3_shots;
pub mod edit_prefs;
pub mod editor_settings;
pub mod eng3_shots;
pub mod export;
pub mod export_actions_shots;
pub mod export_defaults_shots;
mod export_files;
pub mod export_keys_shots;
pub mod export_muted_shots;
pub mod export_selection;
pub mod export_selection_shots;
pub mod export_v1_shots;
pub mod feel2_shots;
pub mod feel_shots;
pub mod file_drop;
pub mod fill_ink_shots;
pub mod fixed_screens_shots;
pub mod folder_band_shots;
pub mod folder_bus;
pub mod follow;
pub mod fonts;
pub mod frame;
pub mod fx_chain;
pub mod fx_preset;
pub mod gain_tip;
pub mod glyphs;
pub mod grid_lines;
pub mod health;
pub mod heavy_plugin_shots;
pub mod history_time_shots;
pub mod hover_tip;
pub mod idle;
pub mod import_choice_shots;
pub mod import_shots;
pub mod inmeter;
pub mod input;
pub mod input_dispatch;
pub mod input_follow;
pub mod input_row;
pub mod inspector;
pub mod inspector_shots;
pub mod instrument;
mod interaction;
mod io_command;
pub mod keep;
pub mod keep_shots;
pub mod key_rows;
pub mod keymap_shots;
pub mod keys_rest_shots;
pub mod keys_view_menu_shots;
pub mod lane_rect_shots;
pub mod lane_shots;
#[cfg(feature = "loading-probe")]
mod live_edit_shots;
pub mod loading;
pub mod local_time;
mod macro_run;
mod manual_update_shots;
pub mod menu_key_shots;
pub mod meter_popup;
pub mod meters;
pub mod metronome_map;
pub mod midi_assign;
mod midi_assign_shots;
pub mod midi_auto_shots;
pub mod midi_drum_shots;
pub mod midi_live;
pub mod midi_open;
pub mod midi_out_row;
pub mod midi_watch;
pub mod mixer;
pub mod mixer_copy_shots;
pub mod mixer_order_shots;
pub mod mixer_shots;
pub mod motion_watch;
pub mod mouse2_shots;
pub mod mouse_cut_shots;
pub mod mouse_dispatch;
pub mod mp3_shots;
pub mod name_drag;
pub mod native_key;
pub mod note_edit_panel;
pub mod notes;
pub mod notice;
#[cfg(test)]
mod open_later_tests;
pub mod opening;
#[cfg(feature = "loading-probe")]
pub mod opening_shots;
pub mod opening_view;
pub mod operations;
pub mod opline;
pub mod opline_shots;
pub mod other_change;
pub mod otherproc_shots;
pub mod overlap2_shots;
pub mod overlap_shots;
pub mod paint;
pub mod palette;
pub mod palette_args;
pub mod pan;
pub mod park;
pub mod peaks;
pub mod pick_list;
pub mod plugin_crash_card;
pub mod plugin_crash_shots;
pub mod plugin_down_shots;
pub mod plugin_fixture_shots;
pub mod plugin_keys;
pub mod plugin_keys_shots;
pub mod plugin_recovery_shots;
pub mod plugin_reload_shots;
pub mod plugin_shots;
pub mod plugin_swap_shots;
pub mod pointer;
#[cfg(windows)]
pub mod pointer_os;
pub mod pointer_shots;
pub mod polish_shots;
pub mod pool_shots;
pub mod pool_tidy_shots;
pub mod popup;
pub mod prepare_card;
#[cfg(test)]
mod prepare_card_tests;
pub mod prepare_shots;
#[cfg(feature = "loading-probe")]
mod prepared_shots;
pub mod quantize_import;
pub mod quantize_panel;
pub mod quiet_shots;
pub mod range_trim_shots;
pub mod rec_area;
pub mod rec_panel;
pub mod rec_shots;
mod recent_worker;
pub mod record_loss;
pub mod record_loss_shots;
pub mod recovery_card;
pub mod recux_shots;
pub mod rename_undo_shots;
pub mod repaint_shots;
#[cfg(test)]
mod review_io_tests;
pub mod ripple_shots;
pub mod roll;
pub mod rollruler_shots;
pub mod ruler;
pub mod ruler_drag;
pub mod ruler_drag_shots;
pub mod saving;
pub mod saving_shots;
pub mod screen;
pub mod screencmd_shots;
pub mod script;
pub mod settings;
pub mod shot;
pub mod shot_more;
pub mod shot_small;
pub mod side_edge;
pub mod side_shots;
pub mod smf;
#[cfg(test)]
mod snap_length_tests;
pub mod song;
pub mod spatial;
pub mod start;
pub mod startup_shots;
pub mod studio;
pub mod tempo_edit_shots;
pub mod tempo_follow_shots;
pub mod theme;
pub mod tidy;
pub mod title_bar;
pub mod toast;
pub mod toast_hover_shots;
pub mod tool_cursor;
pub mod tools;
pub mod topbar;
pub mod track_group;
pub mod track_name;
pub mod track_select;
pub mod trackmenu_shots;
pub mod transport;
pub mod tree;
mod ui_io;
mod unsaved;
pub mod update_shots;
pub mod view_motion;
pub mod viewport;
pub mod wait;
pub mod wait_shots;
pub mod wave;
pub mod wave_mod_shots;
pub mod wave_zoom;
pub mod wave_zoom_shots;
pub(crate) mod wheel;
pub mod wheel_shots;
pub mod zoom_input;

#[cfg(test)]
mod edit3_finish_tests;
#[cfg(test)]
mod polish_tests;
#[cfg(test)]
mod rec_tests;
#[cfg(test)]
mod track_color_tests;
#[cfg(test)]
mod tracklist_tests;
#[cfg(test)]
mod watch_tests;

/// The screenshot scripts `xirius-ui --shot <dir> --script <name>` plays. A script is a file of
/// its own that holds its list of [`shot::Shot`]s and the functions of its stages
/// ([`shot::Act::Stage`]); it is registered by one line here, put in its place by name.
pub const SCRIPTS: &[(&str, &[shot::Shot])] = &[
    ("about", about_shots::ABOUT),
    ("about-crates", about_crates_shots::SHOTS),
    ("ai", ai::shots::AI),
    ("aiarc", aiarc_shots::AIARC),
    ("all", shot::SCRIPT),
    ("arrange-v1", arrange_v1_shots::SHOTS),
    ("audioproc", audioproc_shots::AUDIOPROC),
    ("audioset", audioset_shots::AUDIOSET),
    ("automation-modes", automation_modes_shots::SHOTS),
    ("automation-pdc", automation_pdc_shots::SHOTS),
    ("automation-point-edit", automation_point_edit_shots::SHOTS),
    (
        "automation-preferences",
        automation_preferences_shots::SHOTS,
    ),
    ("automation-send", automation_send_shots::SHOTS),
    ("automation-targets", automation_targets_shots::SHOTS),
    ("automation-track-mode", automation_track_mode_shots::SHOTS),
    ("bar-states", fixed_screens_shots::BAR_STATES),
    ("bartools", bar_tools_shots::SHOTS),
    ("browser-builtins", browser_builtin_shots::SHOTS),
    ("browser-files-menu", browser_files_menu_shots::SHOTS),
    ("bugs2", bugs2_shots::SHOTS),
    ("caption", caption_shots::CAPTION),
    ("channel-step", channel_step_shots::SHOTS),
    ("clip-menu-rows", clip_menu_rows_shots::SHOTS),
    ("clipedit", clipedit_shots::CLIPEDIT),
    ("controls-macros", controls_shots::MACROS),
    ("controls-mouse", controls_shots::MOUSE),
    ("controls-presets", controls_shots::PRESETS),
    ("controls-search", controls_shots::SEARCH),
    ("cpu-ping", cpu_ping_shots::SHOTS),
    ("crash-entry", crash_entry_shots::SHOTS),
    ("ctx", ctx_shots::CTX),
    ("ctx2", ctx2_shots::CTX2),
    ("ctxfolder", ctxfolder_shots::CTXFOLDER),
    ("ctxnames", ctxnames_shots::CTXNAMES),
    ("ctxshape", ctxshape_shots::CTXSHAPE),
    ("dawproject", dawproject_shots::SHOTS),
    ("dawproject-reference", dawproject_reference_shots::SHOTS),
    ("dblclick", dblclick_shots::SHOTS),
    ("draw-length", draw_length_shots::SHOTS),
    ("drawer", drawer_shots::DRAWER),
    ("drum-names", drum_names_shots::SHOTS),
    ("drum-rows", drum_rows_shots::SHOTS),
    ("drum-view", drum_view_shots::SHOTS),
    ("e2e", e2e_shots::SHOTS),
    ("edit3", edit3_shots::EDIT3),
    ("eng3", eng3_shots::ENG3),
    ("export-actions", export_actions_shots::SHOTS),
    ("export-defaults", export_defaults_shots::SHOTS),
    ("export-keys", export_keys_shots::SHOTS),
    ("export-muted", export_muted_shots::SHOTS),
    ("export-selection", export_selection_shots::SHOTS),
    ("export-v1", export_v1_shots::SHOTS),
    ("feel", feel_shots::SHOTS),
    ("feel2", feel2_shots::SHOTS),
    ("fill-ink", fill_ink_shots::SHOTS),
    ("folder-band", folder_band_shots::SHOTS),
    ("heavy-plugin", heavy_plugin_shots::SHOTS),
    ("history-time", history_time_shots::SHOTS),
    ("import", import_shots::IMPORT),
    ("importchoice", import_choice_shots::IMPORT_CHOICE),
    ("inspector", inspector_shots::SHOTS),
    ("instrument", instrument::shots::SHOTS),
    ("instrument-menu", fixed_screens_shots::INSTRUMENT_MENU),
    ("keep", keep_shots::KEEP),
    ("keymap", keymap_shots::SHOTS),
    ("keys-rest", keys_rest_shots::SHOTS),
    ("keys-view-menu", keys_view_menu_shots::SHOTS),
    ("lane-rect", lane_rect_shots::SHOTS),
    ("lanes", lane_shots::LANES),
    ("menu-key", menu_key_shots::SHOTS),
    ("midi-assign", midi_assign_shots::SHOTS),
    ("midi-auto", midi_auto_shots::SHOTS),
    ("midi-drum", midi_drum_shots::SHOTS),
    ("mixer", mixer_shots::MIXER),
    ("mixer-copy", mixer_copy_shots::SHOTS),
    ("mixer-order", mixer_order_shots::SHOTS),
    ("more", shot_more::MORE),
    ("mouse-cut", mouse_cut_shots::SHOTS),
    ("mouse2", mouse2_shots::MOUSE2),
    ("mp3", mp3_shots::MP3),
    #[cfg(feature = "loading-probe")]
    ("opening", opening_shots::SHOTS),
    ("opline", opline_shots::OPLINE),
    ("otherproc", otherproc_shots::SHOTS),
    ("overlap", overlap_shots::SHOTS),
    ("overlap2", overlap2_shots::SHOTS),
    ("palette", palette::SHOTS),
    ("plugin", plugin_shots::SHOTS),
    ("plugin-crash", plugin_crash_shots::SHOTS),
    ("plugin-down", plugin_down_shots::SHOTS),
    ("plugin-fixture", plugin_fixture_shots::SHOTS),
    ("plugin-keys", plugin_keys_shots::SHOTS),
    ("plugin-recovery", plugin_recovery_shots::SHOTS),
    ("plugin-reload", plugin_reload_shots::SHOTS),
    ("plugin-swap", plugin_swap_shots::SHOTS),
    ("pointer", pointer_shots::SHOTS),
    ("polish", polish_shots::SHOTS),
    ("pool", pool_shots::SHOTS),
    ("pool-tidy", pool_tidy_shots::SHOTS),
    ("prepare", prepare_shots::SHOTS),
    #[cfg(feature = "loading-probe")]
    ("prepared", prepared_shots::SHOTS),
    #[cfg(feature = "loading-probe")]
    ("live-edit", live_edit_shots::SHOTS),
    ("quiet", quiet_shots::QUIET),
    ("rail-tips", fixed_screens_shots::RAIL_TIPS),
    ("range-trim", range_trim_shots::SHOTS),
    ("rec", rec_shots::REC),
    ("record-loss", record_loss_shots::SHOTS),
    ("recux", recux_shots::RECUX),
    ("rename-undo", rename_undo_shots::SHOTS),
    ("repaint", repaint_shots::REPAINT),
    ("ripple", ripple_shots::SHOTS),
    ("roll-arrow", fixed_screens_shots::ROLL_ARROW),
    ("rollruler", rollruler_shots::ROLL_RULER),
    ("ruler-drag", ruler_drag_shots::SHOTS),
    ("saving", saving_shots::SHOTS),
    ("screencmd", screencmd_shots::SHOTS),
    ("settings", settings::shots::SHOTS),
    ("side", side_shots::SIDE),
    ("small", shot_small::SMALL),
    ("startup", startup_shots::SHOTS),
    ("tempo-edit", tempo_edit_shots::SHOTS),
    ("tempo-follow", tempo_follow_shots::SHOTS),
    ("toast-hover", toast_hover_shots::SHOTS),
    ("trackmenu", trackmenu_shots::TRACKMENU),
    ("update", update_shots::SHOTS),
    ("update-check", manual_update_shots::SHOTS),
    ("wait", wait_shots::WAIT),
    ("wave-mod", wave_mod_shots::SHOTS),
    ("wave-zoom", wave_zoom_shots::SHOTS),
    ("wheel", wheel_shots::SHOTS),
    ("wire", shot::WIRE),
];
#[cfg(test)]
mod save_crash_tests;
