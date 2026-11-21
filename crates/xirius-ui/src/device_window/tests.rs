use super::*;

#[test]
fn a_work_area_smaller_than_the_device_minimum_is_a_stable_resize_target() {
    let def = &xirius_params::SAMPLER;
    let work = Some((200.0, 150.0));
    let mut body = (800.0, 600.0);
    for _ in 0..12 {
        let rendered = bounded_body(body, work);
        let observed = bounded_body(crate::instrument::window::fit_of(def, rendered, None), work);
        assert_eq!(rendered, observed);
        body = observed;
    }
}

#[test]
fn an_independent_device_can_grow_beyond_its_current_viewport() {
    use crate::instrument::{Pick, current, window::Part};
    let mut s = Studio::sample();
    let track = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    assert!(s.set_instrument(track, &Pick::built_in(crate::instrument::DEFAULT)));
    let id = current(&s.project().tracks[&track]).unwrap().id;
    s.show_instrument(track, id);
    let before = s.instruments.open[0].body;
    let old = scope(&mut s, Some(id));
    let g = surface_geometry(&s, before.0, before.1 + crate::title_bar::BAR_H);
    let (x, y) = s.instrument_center(&g, id, Part::Resize).unwrap();
    assert!(s.instrument_down(&g, x, y));
    assert!(s.instrument_moved(&g, x + 120.0, y + 80.0, false));
    s.instrument_up();
    assert!(s.instruments.open[0].body.0 > before.0);
    assert!(s.instruments.open[0].body.1 > before.1);
    restore(&mut s, old);
}

#[test]
fn independent_surfaces_leave_main_viewport_and_animation_targets_untouched() {
    let mut s = Studio::sample();
    s.motion.enabled.set(true);
    let main = s.geometry(1280.0, 800.0);
    // Start a real panel transition before opening differently sized editors.
    s.browser.open = !s.browser.open;
    s.geometry(1280.0, 800.0);
    let motion = format!("{:?}", s.motion);
    let viewport = s.viewport_size.get();
    let id = PluginId(1);
    for _ in 0..6 {
        for (target, width, height) in [
            (Some(id), 360.0, 440.0),
            (None, 900.0, 600.0),
            (Some(id), 750.0, 720.0),
        ] {
            let old = scope(&mut s, target);
            let local = surface_geometry(&s, width, height);
            assert_eq!(local.size, (width, height));
            restore(&mut s, old);
            assert_eq!(s.viewport_size.get(), viewport);
            assert_eq!(format!("{:?}", s.motion), motion);
            assert_eq!(s.instruments.detached_target, None);
            assert!(!s.instruments.detached_channel);
        }
    }
    assert_eq!(s.geometry(1280.0, 800.0).size, main.size);
    assert_eq!(format!("{:?}", s.motion), motion);
}

#[test]
fn independent_shortcuts_do_not_inherit_the_main_browser_context() {
    let mut s = Studio::sample();
    let main = s.geometry(1280.0, 800.0);
    s.browser.open = true;
    s.input_map.browser_focus = true;
    assert_eq!(s.input_context(&main), "browser");
    let old = scope(&mut s, Some(PluginId(1)));
    assert_eq!(s.input_context(&main), "global");
    restore(&mut s, old);
    assert_eq!(s.input_context(&main), "browser");
}

#[test]
fn tab_in_an_empty_device_does_not_advance_main_focus_or_layout() {
    let mut s = Studio::sample();
    s.geometry(1280.0, 800.0);
    s.input_map.browser_focus = true;
    let viewport = s.viewport_size.get();
    let motion = format!("{:?}", s.motion);
    let old = scope(&mut s, Some(PluginId(1)));
    let local = surface_geometry(&s, 360.0, 240.0);
    for key in ["tab", "shift-tab"] {
        device_key_event(
            &mut s,
            &local,
            &gpui::KeyDownEvent {
                keystroke: gpui::Keystroke::parse(key).unwrap(),
                is_held: false,
                prefer_character_input: false,
            },
            None,
        );
        assert!(s.input_map.browser_focus);
        assert_eq!(s.instruments.focus, None);
        assert_eq!(s.viewport_size.get(), viewport);
        assert_eq!(format!("{:?}", s.motion), motion);
    }
    restore(&mut s, old);
}

#[test]
fn channel_window_keeps_local_values_but_ignores_main_text_and_browser_enter() {
    use crate::settings::{Field, FieldId};
    let mut s = Studio::sample();
    let track = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    s.open_channel(track);
    s.input_map.name = Some(crate::track_name::Name {
        track,
        field: Field::new(FieldId::Search, "Main".into()),
    });
    s.browser.open = true;
    s.browser.tab = crate::browser::RailTab::Effects;
    s.browser.selected_item = Some("Reverb");
    s.mixer.browser_fx_focus = true;
    let old = scope(&mut s, None);
    let local = surface_geometry(&s, 800.0, 600.0);
    assert_eq!(s.ime_target(), None);
    s.ime_replace(None, "ignored", false);
    assert_eq!(s.input_map.name.as_ref().unwrap().field.text(), "Main");
    assert!(!s.pick_typed(&local, "enter", None));
    let viewport = s.viewport_size.get();
    let motion = format!("{:?}", s.motion);
    for key in ["enter", "escape", "backspace", "tab"] {
        device_key_event(
            &mut s,
            &local,
            &gpui::KeyDownEvent {
                keystroke: gpui::Keystroke::parse(key).unwrap(),
                is_held: false,
                prefer_character_input: false,
            },
            None,
        );
        assert_eq!(s.input_map.name.as_ref().unwrap().field.text(), "Main");
        assert_eq!(s.viewport_size.get(), viewport);
        assert_eq!(format!("{:?}", s.motion), motion);
    }
    s.head_fader.entry = Some(crate::head_fader::Entry {
        track,
        field: Field::new(FieldId::Search, "-3".into()),
        fresh: false,
    });
    assert_eq!(s.ime_target(), Some(crate::interaction::ImeTarget::Head));
    s.ime_replace(None, ".5", false);
    assert_eq!(s.head_fader.entry.as_ref().unwrap().field.text(), "-3.5");
    restore(&mut s, old);
}

/// A field in an independent window takes its letter once: the key event's
/// own path puts nothing in a field the IME owns — `WM_CHAR` comes through
/// the `EntityInputHandler` (UI-338).
#[test]
fn a_channel_window_field_takes_a_letter_once() {
    use crate::settings::{Field, FieldId};
    let mut s = Studio::sample();
    let track = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    s.open_channel(track);
    let old = scope(&mut s, None);
    let local = surface_geometry(&s, 800.0, 600.0);
    s.head_fader.entry = Some(crate::head_fader::Entry {
        track,
        field: Field::new(FieldId::Search, "-3".into()),
        fresh: false,
    });
    assert_eq!(s.ime_target(), Some(crate::interaction::ImeTarget::Head));
    device_key_event(
        &mut s,
        &local,
        &gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse("k->k").unwrap(),
            is_held: false,
            prefer_character_input: false,
        },
        None,
    );
    assert_eq!(s.head_fader.entry.as_ref().unwrap().field.text(), "-3");
    s.ime_replace(None, "k", false);
    assert_eq!(s.head_fader.entry.as_ref().unwrap().field.text(), "-3k");
    restore(&mut s, old);
}

#[test]
fn independent_ime_does_not_edit_a_main_field_or_another_device() {
    use crate::settings::{Field, FieldId};
    let mut s = Studio::sample();
    s.input_map.name = Some(crate::track_name::Name {
        track: xirius_core::id::TrackId::MASTER,
        field: Field::new(FieldId::Search, "Main".into()),
    });
    s.instruments.entry = Some(crate::device::input::Entry {
        plugin: PluginId(2),
        k: 0,
        field: Field::new(FieldId::Search, "Other".into()),
        fresh: false,
    });
    let old = scope(&mut s, Some(PluginId(1)));
    assert_eq!(s.ime_target(), None);
    s.ime_replace(None, "ignored", false);
    assert_eq!(s.input_map.name.as_ref().unwrap().field.text(), "Main");
    assert_eq!(s.instruments.entry.as_ref().unwrap().field.text(), "Other");
    s.instruments.entry.as_mut().unwrap().plugin = PluginId(1);
    assert_eq!(s.ime_target(), Some(crate::interaction::ImeTarget::Device));
    s.ime_replace(None, " local", false);
    assert_eq!(
        s.instruments.entry.as_ref().unwrap().field.text(),
        "Other local"
    );
    restore(&mut s, old);
    assert_eq!(s.ime_target(), Some(crate::interaction::ImeTarget::Name));
}

mod headless {
    use gpui::{
        Action, ActivityGuard, AnyWindowHandle, BackgroundExecutor, Bounds, Capslock,
        ClipboardItem, CursorStyle, DispatchEventResult, DisplayId, DummyKeyboardMapper,
        ForegroundExecutor, GpuSpecs, HeadlessAtlas, Keymap, Modifiers, NoopTextSystem,
        PathPromptOptions, Pixels, Platform, PlatformAtlas, PlatformDispatcher, PlatformDisplay,
        PlatformInput, PlatformInputHandler, PlatformKeyboardLayout, PlatformKeyboardMapper,
        PlatformTextSystem, PlatformWindow, Point, Priority, PromptButton, PromptLevel,
        RequestFrameOptions, RunnableVariant, Scene, Size, Task, ThermalState, WindowAppearance,
        WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowParams,
        WindowVisibility, point, px, size,
    };
    use raw_window_handle::{
        DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
    };
    use std::{
        cell::{Cell, RefCell},
        ffi::OsString,
        path::{Path, PathBuf},
        rc::Rc,
        sync::Arc,
        time::Duration,
    };

    struct InlineDispatcher;

    impl PlatformDispatcher for InlineDispatcher {
        fn is_main_thread(&self) -> bool {
            true
        }
        fn dispatch(&self, runnable: RunnableVariant, _: Priority) {
            runnable.run();
        }
        fn dispatch_on_main_thread(&self, runnable: RunnableVariant, _: Priority) {
            runnable.run();
        }
        fn dispatch_after(&self, _: Duration, _: RunnableVariant) {}
        fn spawn_realtime(&self, f: Box<dyn FnOnce() + Send>) {
            f();
        }
    }

    #[derive(Debug)]
    struct HeadlessDisplay;

    impl PlatformDisplay for HeadlessDisplay {
        fn id(&self) -> DisplayId {
            DisplayId::new(1)
        }
        fn uuid(&self) -> gpui::Result<uuid::Uuid> {
            Ok(uuid::Uuid::nil())
        }
        fn bounds(&self) -> Bounds<Pixels> {
            Bounds::new(point(px(0.0), px(0.0)), size(px(1920.0), px(1080.0)))
        }
    }

    struct DummyLayout;

    impl PlatformKeyboardLayout for DummyLayout {
        fn id(&self) -> &str {
            "us"
        }
        fn name(&self) -> &str {
            "US"
        }
    }

    type FrameRequest = Box<dyn FnMut(RequestFrameOptions)>;
    type InputCallback = Box<dyn FnMut(PlatformInput) -> DispatchEventResult>;

    pub(super) struct WindowState {
        bounds: Cell<Bounds<Pixels>>,
        display: Rc<dyn PlatformDisplay>,
        atlas: Arc<dyn PlatformAtlas>,
        request_frame: RefCell<Option<FrameRequest>>,
        input: RefCell<Option<InputCallback>>,
        input_handler: RefCell<Option<PlatformInputHandler>>,
    }

    #[derive(Clone)]
    pub(super) struct HeadlessWindow(pub(super) Rc<WindowState>);

    impl HasWindowHandle for HeadlessWindow {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            Err(HandleError::NotSupported)
        }
    }

    impl HasDisplayHandle for HeadlessWindow {
        fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
            Err(HandleError::NotSupported)
        }
    }

    impl HeadlessWindow {
        pub(super) fn draw(&self) {
            let cb = self.0.request_frame.borrow_mut().take();
            if let Some(mut cb) = cb {
                cb(RequestFrameOptions {
                    force_render: true,
                    ..Default::default()
                });
                *self.0.request_frame.borrow_mut() = Some(cb);
            }
        }

        pub(super) fn dispatch(&self, ev: PlatformInput) {
            let cb = self.0.input.borrow_mut().take();
            if let Some(mut cb) = cb {
                cb(ev);
                *self.0.input.borrow_mut() = Some(cb);
            }
        }
    }

    impl PlatformWindow for HeadlessWindow {
        fn bounds(&self) -> Bounds<Pixels> {
            self.0.bounds.get()
        }
        fn is_maximized(&self) -> bool {
            false
        }
        fn window_bounds(&self) -> WindowBounds {
            WindowBounds::Windowed(self.0.bounds.get())
        }
        fn content_size(&self) -> Size<Pixels> {
            self.0.bounds.get().size
        }
        fn resize(&mut self, size: Size<Pixels>) {
            let mut b = self.0.bounds.get();
            b.size = size;
            self.0.bounds.set(b);
        }
        fn scale_factor(&self) -> f32 {
            1.0
        }
        fn appearance(&self) -> WindowAppearance {
            WindowAppearance::Dark
        }
        fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
            Some(self.0.display.clone())
        }
        fn mouse_position(&self) -> Point<Pixels> {
            Point::default()
        }
        fn modifiers(&self) -> Modifiers {
            Modifiers::default()
        }
        fn capslock(&self) -> Capslock {
            Capslock::default()
        }
        fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
            *self.0.input_handler.borrow_mut() = Some(input_handler);
        }
        fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
            self.0.input_handler.borrow_mut().take()
        }
        fn prompt(
            &self,
            _: PromptLevel,
            _: &str,
            _: Option<&str>,
            _: &[PromptButton],
        ) -> Option<futures_channel::oneshot::Receiver<usize>> {
            None
        }
        fn activate(&self) {}
        fn is_active(&self) -> bool {
            true
        }
        fn visibility(&self) -> WindowVisibility {
            WindowVisibility::Visible
        }
        fn is_hovered(&self) -> bool {
            true
        }
        fn background_appearance(&self) -> WindowBackgroundAppearance {
            WindowBackgroundAppearance::Opaque
        }
        fn set_title(&mut self, _: &str) {}
        fn set_background_appearance(&self, _: WindowBackgroundAppearance) {}
        fn minimize(&self) {}
        fn zoom(&self) {}
        fn toggle_fullscreen(&self) {}
        fn is_fullscreen(&self) -> bool {
            false
        }
        fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
            *self.0.request_frame.borrow_mut() = Some(callback);
        }
        fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
            *self.0.input.borrow_mut() = Some(callback);
        }
        fn on_active_status_change(&self, _: Box<dyn FnMut(bool)>) {}
        fn on_visibility_change(&self, _: Box<dyn FnMut(WindowVisibility)>) {}
        fn on_hover_status_change(&self, _: Box<dyn FnMut(bool)>) {}
        fn on_resize(&self, _: Box<dyn FnMut(Size<Pixels>, f32)>) {}
        fn on_moved(&self, _: Box<dyn FnMut()>) {}
        fn on_should_close(&self, _: Box<dyn FnMut() -> bool>) {}
        fn on_hit_test_window_control(&self, _: Box<dyn FnMut() -> Option<WindowControlArea>>) {}
        fn on_close(&self, _: Box<dyn FnOnce()>) {}
        fn on_appearance_changed(&self, _: Box<dyn FnMut()>) {}
        fn draw(&self, _: &Scene) {}
        fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
            self.0.atlas.clone()
        }
        fn is_subpixel_rendering_supported(&self) -> bool {
            false
        }
        #[cfg(target_os = "windows")]
        fn get_raw_handle(&self) -> windows::Win32::Foundation::HWND {
            windows::Win32::Foundation::HWND(std::ptr::null_mut())
        }
        fn gpu_specs(&self) -> Option<GpuSpecs> {
            None
        }
        fn update_ime_position(&self, _: Bounds<Pixels>) {}
    }

    pub(super) struct HeadlessPlatform {
        background: BackgroundExecutor,
        foreground: ForegroundExecutor,
        text_system: Arc<dyn PlatformTextSystem>,
        display: Rc<dyn PlatformDisplay>,
        pub(super) windows: RefCell<Vec<HeadlessWindow>>,
    }

    impl HeadlessPlatform {
        pub(super) fn new() -> Rc<Self> {
            let dispatcher: Arc<dyn PlatformDispatcher> = Arc::new(InlineDispatcher);
            Rc::new(Self {
                background: BackgroundExecutor::new(dispatcher.clone()),
                foreground: ForegroundExecutor::new(dispatcher),
                text_system: Arc::new(NoopTextSystem::new()),
                display: Rc::new(HeadlessDisplay),
                windows: RefCell::new(Vec::new()),
            })
        }
    }

    impl Platform for HeadlessPlatform {
        fn background_executor(&self) -> BackgroundExecutor {
            self.background.clone()
        }
        fn foreground_executor(&self) -> ForegroundExecutor {
            self.foreground.clone()
        }
        fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
            self.text_system.clone()
        }
        fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
            on_finish_launching();
        }
        fn quit(&self) {}
        fn restart(&self, _: Option<PathBuf>, _: Vec<OsString>) {}
        fn activate(&self, _: bool) {}
        fn hide(&self) {}
        fn hide_other_apps(&self) {}
        fn unhide_other_apps(&self) {}
        fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
            vec![self.display.clone()]
        }
        fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
            Some(self.display.clone())
        }
        fn active_window(&self) -> Option<AnyWindowHandle> {
            None
        }
        fn open_window(
            &self,
            _: AnyWindowHandle,
            options: WindowParams,
        ) -> gpui::Result<Box<dyn PlatformWindow>> {
            let win = HeadlessWindow(Rc::new(WindowState {
                bounds: Cell::new(options.bounds),
                display: self.display.clone(),
                atlas: Arc::new(HeadlessAtlas::default()),
                request_frame: RefCell::new(None),
                input: RefCell::new(None),
                input_handler: RefCell::new(None),
            }));
            self.windows.borrow_mut().push(win.clone());
            Ok(Box::new(win))
        }
        fn window_appearance(&self) -> WindowAppearance {
            WindowAppearance::Dark
        }
        fn open_url(&self, _: &str) {}
        fn on_open_urls(&self, _: Box<dyn FnMut(Vec<String>)>) {}
        fn register_url_scheme(&self, _: &str) -> Task<gpui::Result<()>> {
            Task::ready(Ok(()))
        }
        fn prompt_for_paths(
            &self,
            _: PathPromptOptions,
        ) -> futures_channel::oneshot::Receiver<gpui::Result<Option<Vec<PathBuf>>>> {
            unimplemented!()
        }
        fn prompt_for_new_path(
            &self,
            _: &Path,
            _: Option<&str>,
        ) -> futures_channel::oneshot::Receiver<gpui::Result<Option<PathBuf>>> {
            unimplemented!()
        }
        fn can_select_mixed_files_and_dirs(&self) -> bool {
            false
        }
        fn reveal_path(&self, _: &Path) {}
        fn open_with_system(&self, _: &Path) {}
        fn on_quit(&self, _: Box<dyn FnMut() -> bool>) {}
        fn on_reopen(&self, _: Box<dyn FnMut()>) {}
        fn on_system_sleep(&self, _: Box<dyn FnMut()>) {}
        fn on_system_wake(&self, _: Box<dyn FnMut()>) {}
        fn set_menus(&self, _: Vec<gpui::Menu>, _: &Keymap) {}
        fn set_dock_menu(&self, _: Vec<gpui::MenuItem>, _: &Keymap) {}
        fn on_app_menu_action(&self, _: Box<dyn FnMut(&dyn Action)>) {}
        fn on_will_open_app_menu(&self, _: Box<dyn FnMut()>) {}
        fn on_validate_app_menu_command(&self, _: Box<dyn FnMut(&dyn Action) -> bool>) {}
        fn thermal_state(&self) -> ThermalState {
            ThermalState::Nominal
        }
        fn on_thermal_state_change(&self, _: Box<dyn FnMut()>) {}
        fn prevent_idle_sleep(&self, _: &str) -> Task<gpui::Result<ActivityGuard>> {
            Task::ready(Ok(ActivityGuard::noop()))
        }
        fn app_path(&self) -> gpui::Result<PathBuf> {
            Ok(PathBuf::new())
        }
        fn path_for_auxiliary_executable(&self, _: &str) -> gpui::Result<PathBuf> {
            Ok(PathBuf::new())
        }
        fn set_cursor_style(&self, _: CursorStyle) {}
        fn hide_cursor_until_mouse_moves(&self) {}
        fn is_cursor_visible(&self) -> bool {
            true
        }
        fn should_auto_hide_scrollbars(&self) -> bool {
            false
        }
        fn read_from_clipboard(&self) -> Option<ClipboardItem> {
            None
        }
        fn write_to_clipboard(&self, _: ClipboardItem) {}
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        fn read_from_primary(&self) -> Option<ClipboardItem> {
            None
        }
        #[cfg(any(target_os = "linux", target_os = "freebsd"))]
        fn write_to_primary(&self, _: ClipboardItem) {}
        #[cfg(target_os = "macos")]
        fn read_from_find_pasteboard(&self) -> Option<ClipboardItem> {
            None
        }
        #[cfg(target_os = "macos")]
        fn write_to_find_pasteboard(&self, _: ClipboardItem) {}
        fn write_credentials(&self, _: &str, _: &str, _: &[u8]) -> Task<gpui::Result<()>> {
            Task::ready(Ok(()))
        }
        fn read_credentials(&self, _: &str) -> Task<gpui::Result<Option<(String, Vec<u8>)>>> {
            Task::ready(Ok(None))
        }
        fn delete_credentials(&self, _: &str) -> Task<gpui::Result<()>> {
            Task::ready(Ok(()))
        }
        fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
            Box::new(DummyLayout)
        }
        fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
            Rc::new(DummyKeyboardMapper)
        }
        fn on_keyboard_layout_change(&self, _: Box<dyn FnMut()>) {}
    }

    pub(super) struct BlankRoot;
    impl gpui::Render for BlankRoot {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            gpui::div()
        }
    }

    /// The main window the `Windows::sync` call under test stands on. `independent::open`
    /// looks the OS's window of a title up to parent and place, which a headless window
    /// has none of: this is the same `cx.open_window` without that look-up.
    pub(super) fn open_test<V: gpui::Render + 'static>(
        cx: &mut gpui::App,
        bounds: Bounds<Pixels>,
        build: impl FnOnce(&mut gpui::Window, &mut gpui::App) -> gpui::Entity<V> + 'static,
    ) -> gpui::Result<gpui::WindowHandle<V>> {
        cx.open_window(
            gpui::WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            build,
        )
    }
}

fn write_test_wav(path: &std::path::Path) {
    let tone: Vec<f32> = (0..441).map(|i| (i as f32 * 0.1).sin() * 0.4).collect();
    xirius_session::export::write_wav(path, &tone, &tone, 44_100, 16).unwrap();
}

#[test]
fn independent_device_window_accepts_external_file_drops_and_clears_preview() {
    use crate::{
        device::sampler::DropOn,
        instrument::{Pick, current, window::Part},
    };
    use gpui::{
        Application, Capslock, ExternalPaths, FileDropEvent, Modifiers, ModifiersChangedEvent,
        PlatformInput, point, px, size,
    };
    use headless::{BlankRoot, HeadlessPlatform};

    let dir = tempfile::tempdir().unwrap();
    let kick = dir.path().join("Kick.wav");
    let snare = dir.path().join("Snare.wav");
    write_test_wav(&kick);
    write_test_wav(&snare);

    let mut s = Studio::sample();
    s.geometry(1280.0, 800.0);
    let track = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    assert!(s.set_instrument(track, &Pick::built_in("Drum Kit")));
    let id = current(&s.project().tracks[&track]).unwrap().id;
    s.show_instrument(track, id);

    let studio = Rc::new(RefCell::new(s));
    let windows = Rc::new(RefCell::new(Windows::default()));
    let platform = HeadlessPlatform::new();

    let _app = Application::with_platform(platform.clone()).run_embedded({
        let studio = studio.clone();
        let windows = windows.clone();
        move |cx| {
            let main = headless::open_test(
                cx,
                Bounds::new(point(px(0.0), px(0.0)), size(px(1280.0), px(800.0))),
                |_, cx| cx.new(|_| BlankRoot),
            )
            .unwrap();
            let _ = main.update(cx, |_, main_win, cx| {
                windows.borrow_mut().sync(&studio, main_win, cx);
            });
        }
    });

    let dev_win = platform.windows.borrow()[1].clone();
    dev_win.draw();

    let (pad_pt, plane_pt) = {
        let mut s = studio.borrow_mut();
        let body = s.instruments.open[0].body;
        let old = scope(&mut s, Some(id));
        let g = surface_geometry(&s, body.0, body.1 + crate::title_bar::BAR_H);
        let pad = s.instrument_center(&g, id, Part::Pad(36)).unwrap();
        let plane = s.instrument_center(&g, id, Part::Body).unwrap();
        restore(&mut s, old);
        (point(px(pad.0), px(pad.1)), point(px(plane.0), px(plane.1)))
    };

    let mut paths = ExternalPaths::default();
    paths.0.extend([kick.clone(), snare.clone()]);

    // 1. Enter over empty pad 0 -> preview and DropOn::Plane (while kit has no samples) are set.
    dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Entered {
        position: pad_pt,
        paths: paths.clone(),
    }));
    {
        let s = studio.borrow();
        assert_eq!(
            s.instruments.drop_on.map(|(p, on, _)| (p, on)),
            Some((id, DropOn::Plane))
        );
        let preview = s
            .instruments
            .preview
            .as_ref()
            .expect("FileDrop::Entered on DeviceWindow should populate preview");
        assert_eq!(preview.plugin, id);
        assert_eq!(preview.pads.len(), 2);
        assert!(!s.instruments.drop_layers);
    }

    // 2. Move over Drop plane -> DropOn::Plane is set.
    dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Pending {
        position: plane_pt,
    }));
    assert_eq!(
        studio
            .borrow()
            .instruments
            .drop_on
            .map(|(p, on, _)| (p, on)),
        Some((id, DropOn::Plane))
    );

    // 3. Move back to pad 0 and toggle Shift -> drop_layers updates and stacks onto 1 pad.
    dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Pending {
        position: pad_pt,
    }));
    dev_win.dispatch(PlatformInput::ModifiersChanged(ModifiersChangedEvent {
        modifiers: Modifiers {
            shift: true,
            ..Default::default()
        },
        capslock: Capslock::default(),
    }));
    {
        let s = studio.borrow();
        assert!(s.instruments.drop_layers);
        let preview = s.instruments.preview.as_ref().unwrap();
        assert_eq!(preview.pads.len(), 1);
        assert_eq!(preview.pads[0].2, "Vel 2");
    }
    dev_win.dispatch(PlatformInput::ModifiersChanged(ModifiersChangedEvent {
        modifiers: Modifiers::default(),
        capslock: Capslock::default(),
    }));
    assert!(!studio.borrow().instruments.drop_layers);

    // 4. Drag exits window -> next frame clears preview and drop_on.
    dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Exited));
    dev_win.draw();
    {
        let s = studio.borrow();
        assert_eq!(s.instruments.drop_on, None);
        assert!(s.instruments.preview.is_none());
    }

    // 5. While input_blocked() is true, Entered and Submit are ignored.
    studio.borrow_mut().transport.finishing_take = true;
    dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Entered {
        position: pad_pt,
        paths: paths.clone(),
    }));
    dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Submit {
        position: pad_pt,
    }));
    {
        let mut s = studio.borrow_mut();
        s.transport.finishing_take = false;
        assert_eq!(s.instruments.drop_on, None);
        assert!(s.instruments.preview.is_none());
        assert!(
            current(&s.project().tracks[&track])
                .unwrap()
                .samples
                .is_empty()
        );
    }

    // 6. Real drop (Entered + Submit) loads both WAV files into the Drum Kit pads and supports single-step undo.
    dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Entered {
        position: pad_pt,
        paths,
    }));
    dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Submit {
        position: pad_pt,
    }));
    {
        let mut s = studio.borrow_mut();
        assert_eq!(s.instruments.drop_on, None);
        assert!(s.instruments.preview.is_none());
        let loaded = current(&s.project().tracks[&track])
            .unwrap()
            .samples
            .clone();
        assert_eq!(loaded.len(), 2);
        assert!(loaded[0].ends_with("Kick.wav"));
        assert!(loaded[1].ends_with("Snare.wav"));
        let g = s.geometry(1280.0, 800.0);
        assert!(s.undo(&g));
        assert!(
            current(&s.project().tracks[&track])
                .unwrap()
                .samples
                .is_empty()
        );
    }
}

#[test]
fn every_sample_bearing_builtin_device_window_accepts_external_file_drops() {
    use crate::instrument::{Pick, current, window::Part};
    use gpui::{Application, ExternalPaths, FileDropEvent, PlatformInput, point, px, size};
    use headless::{BlankRoot, HeadlessPlatform};

    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("Tone.wav");
    write_test_wav(&wav);

    let sample_defs: Vec<&xirius_params::BuiltinDef> = xirius_params::BUILTINS
        .iter()
        .copied()
        .filter(|def| crate::device::sampler::has_files(def))
        .collect();
    assert!(
        sample_defs.len() >= 2,
        "expected at least Sampler and Drum Kit to accept sample files"
    );

    for def in sample_defs {
        let mut s = Studio::sample();
        s.geometry(1280.0, 800.0);
        let track = s
            .project()
            .tracks
            .values()
            .find(|t| t.name == "Keys")
            .unwrap()
            .id;
        assert!(s.set_instrument(track, &Pick::built_in(def.name)));
        let id = current(&s.project().tracks[&track]).unwrap().id;
        s.show_instrument(track, id);

        let studio = Rc::new(RefCell::new(s));
        let windows = Rc::new(RefCell::new(Windows::default()));
        let platform = HeadlessPlatform::new();

        let _app = Application::with_platform(platform.clone()).run_embedded({
            let studio = studio.clone();
            let windows = windows.clone();
            move |cx| {
                let main = headless::open_test(
                    cx,
                    Bounds::new(point(px(0.0), px(0.0)), size(px(1280.0), px(800.0))),
                    |_, cx| cx.new(|_| BlankRoot),
                )
                .unwrap();
                let _ = main.update(cx, |_, main_win, cx| {
                    windows.borrow_mut().sync(&studio, main_win, cx);
                });
            }
        });

        let dev_win = platform.windows.borrow()[1].clone();
        dev_win.draw();

        let drop_pt = {
            let mut s = studio.borrow_mut();
            let body = s.instruments.open[0].body;
            let old = scope(&mut s, Some(id));
            let g = surface_geometry(&s, body.0, body.1 + crate::title_bar::BAR_H);
            let pt = s.instrument_center(&g, id, Part::Body).unwrap();
            restore(&mut s, old);
            point(px(pt.0), px(pt.1))
        };

        let mut paths = ExternalPaths::default();
        paths.0.push(wav.clone());
        dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Entered {
            position: drop_pt,
            paths,
        }));
        assert!(
            studio.borrow().instruments.drop_on.is_some(),
            "built-in device `{}` ({}) did not highlight on FileDrop::Entered in DeviceWindow",
            def.uid,
            def.name
        );

        dev_win.dispatch(PlatformInput::FileDrop(FileDropEvent::Submit {
            position: drop_pt,
        }));
        let loaded = current(&studio.borrow().project().tracks[&track])
            .unwrap()
            .samples
            .clone();
        assert_eq!(
            loaded.len(),
            1,
            "built-in device `{}` ({}) did not load dropped sample in DeviceWindow",
            def.uid,
            def.name
        );
    }
}

/// A row dragged by the main window's own mouse events — `Screen::pressed` →
/// `Screen::moved` → `Windows::over` → `browser_row_over`, then `released` →
/// `file_up` — lands on the pad under the pointer, not the pad the device window's
/// remembered mouse position would name (UI-353). The pointer's place inside the
/// window is computed each event; a position the device window saw before does not
/// enter it.
#[test]
fn main_window_events_drop_on_the_pad_under_the_pointer() {
    use crate::browser::{BrowserHit, RailTab};
    use crate::device::sampler::{DropOn, pad_files};
    use crate::import_shots::{file_row, make_place};
    use crate::instrument::{Pick, current, window::Part};
    use gpui::{
        Application, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput,
        PlatformWindow, point, px, size,
    };
    use headless::HeadlessPlatform;

    let dir = tempfile::tempdir().unwrap();
    let top = dir.path().join("素材");
    make_place(&top);

    let mut s = Studio::sample();
    s.browser.press_rail(RailTab::Files);
    s.browser.add_place_fixture(&top).unwrap();
    s.browser.toggle_dir(&top.join("Loops").to_string_lossy());
    crate::browser_files::settle_rows(&s.browser);
    let track = s
        .project()
        .tracks
        .values()
        .find(|t| t.name == "Keys")
        .unwrap()
        .id;
    assert!(s.set_instrument(track, &Pick::built_in("Drum Kit")));
    let id = current(&s.project().tracks[&track]).unwrap().id;
    s.show_instrument(track, id);
    // The device window stands over the arrangement's side of the main window.
    s.instruments.positions.borrow_mut().insert(
        crate::independent::Key::Device(id),
        Bounds::new(point(px(560.0), px(80.0)), size(px(560.0), px(520.0))),
    );
    let tracks = s.project().tracks.len();
    let clips: usize = s.project().tracks.values().map(|t| t.clips.len()).sum();
    // Where the file row sits in the main window's body, as `Screen` lays it out.
    let row_at = {
        let g = s.geometry(
            1280.0,
            800.0 - xirius_design::Layout::TOP_BAR_H - xirius_design::Layout::BOTTOM_BAR_H,
        );
        let i = file_row(&s, &g, "pad_44k.wav").expect("the wav row");
        let l = s.browser.layout(g.panel);
        s.browser.center_of(&l, BrowserHit::File(i)).unwrap()
    };

    let studio = Rc::new(RefCell::new(s));
    let platform = HeadlessPlatform::new();
    let _app = Application::with_platform(platform.clone()).run_embedded({
        let studio = studio.clone();
        move |cx| {
            headless::open_test(
                cx,
                Bounds::new(point(px(0.0), px(0.0)), size(px(1280.0), px(800.0))),
                move |window, cx| crate::screen::Screen::for_test(studio.clone(), window, cx),
            )
            .unwrap();
        }
    });

    let main_win = platform.windows.borrow()[0].clone();
    main_win.draw();
    let dev_win = platform.windows.borrow()[1].clone();
    dev_win.draw();
    let dev_origin = dev_win.bounds().origin;
    assert_eq!(f32::from(dev_origin.x), 560.0);
    // A point the main window's event carries for the device window's local `at`.
    let screen = |at: (f32, f32)| {
        point(
            px(f32::from(dev_origin.x) + at.0),
            px(f32::from(dev_origin.y) + at.1),
        )
    };
    let mods = gpui::Modifiers::default();

    // The pads the drag passes over, in the device window's own coordinates.
    let (pad_a, pad_b, pad_seed) = {
        let mut s = studio.borrow_mut();
        let old = scope(&mut s, Some(id));
        let body = s.instruments.open[0].body;
        let g = surface_geometry(&s, body.0, body.1 + crate::title_bar::BAR_H);
        let (a, b, c) = (
            s.instrument_center(&g, id, Part::Pad(36)).unwrap(),
            s.instrument_center(&g, id, Part::Pad(40)).unwrap(),
            s.instrument_center(&g, id, Part::Pad(44)).unwrap(),
        );
        restore(&mut s, old);
        (a, b, c)
    };
    // A position the device window saw once at `pad_seed`: it must not answer for the
    // drag's pointer (its remembered `mouse_position` would be stale exactly so).
    dev_win.dispatch(PlatformInput::MouseMove(MouseMoveEvent {
        position: point(px(pad_seed.0), px(pad_seed.1)),
        modifiers: mods,
        pressed_button: None,
    }));

    // The press on the row, the moves onto the two pads, the let-go — all on the
    // main window; the device window takes no more input for the whole drag.
    main_win.dispatch(PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position: point(
            px(row_at.0),
            px(row_at.1 + xirius_design::Layout::TOP_BAR_H),
        ),
        modifiers: mods,
        click_count: 1,
        first_mouse: false,
    }));
    assert!(studio.borrow().browser.file_drag.is_some());

    main_win.dispatch(PlatformInput::MouseMove(MouseMoveEvent {
        position: screen(pad_a),
        modifiers: mods,
        pressed_button: Some(MouseButton::Left),
    }));
    {
        let s = studio.borrow();
        assert!(
            matches!(s.over, Over::Window(Kind::Device(over_id), _) if over_id == id),
            "over: {:?}",
            s.over
        );
        // The light follows the pointer: the pad's ghost on pad 0 (the chosen one,
        // so its plane lights too), not the seeded pad 8.
        let preview = s.instruments.preview.as_ref().expect("a drag's plan");
        assert_eq!(preview.pads[0].0, 0);
        assert_eq!(
            s.instruments.drop_on.map(|(p, on, _)| (p, on)),
            Some((id, DropOn::Plane)),
            "the chosen pad's plane lights"
        );
    }
    main_win.dispatch(PlatformInput::MouseMove(MouseMoveEvent {
        position: screen(pad_b),
        modifiers: mods,
        pressed_button: Some(MouseButton::Left),
    }));
    {
        let s = studio.borrow();
        // Now on pad 4, not the chosen pad: the ghost moved there and the chosen
        // pad's plane does not light for it (UI-353).
        let preview = s.instruments.preview.as_ref().expect("a drag's plan");
        assert_eq!(preview.pads[0].0, 4, "the light moved with the pointer");
        assert_eq!(
            s.instruments.drop_on, None,
            "another pad's plane stays dark"
        );
    }
    main_win.dispatch(PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position: screen(pad_b),
        modifiers: mods,
        click_count: 1,
    }));
    {
        let mut s = studio.borrow_mut();
        crate::shot::run_for(&mut s, 10.0, |s| !s.input_blocked());
        s.settle_browser_import();
        s.browser.settle_prefs_io();
        s.follow_browser();
        assert_eq!(s.error, None, "{:?}", s.error);
        assert_eq!(s.over, Over::Main, "over ends with the drag");
        let p = s.project();
        assert_eq!(p.tracks.len(), tracks, "the lanes take nothing");
        assert_eq!(
            p.tracks.values().map(|t| t.clips.len()).sum::<usize>(),
            clips
        );
        let x = current(&p.tracks[&track]).cloned().unwrap();
        assert_eq!(
            pad_files(x.builtin().unwrap(), &x, 4).len(),
            1,
            "the pad under the pointer has the file"
        );
        assert!(pad_files(x.builtin().unwrap(), &x, 0).is_empty());
        assert!(s.browser.file_drag.is_none());
    }
}
