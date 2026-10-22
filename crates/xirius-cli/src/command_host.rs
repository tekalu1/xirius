//! A standalone engine/session controlled through a versioned byte pipe.
pub fn serve() -> std::io::Result<()> {
    let mut session = xirius_core::Session::with_registry(
        xirius_core::Project::new("engine"),
        xirius_session::commands::shared_registry(),
    );
    xirius_session::commands::attach(&mut session, xirius_session::ext::ExtConfig::default());
    xirius_mcp::ipc::serve(
        &std::sync::Mutex::new(session),
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    )
}
