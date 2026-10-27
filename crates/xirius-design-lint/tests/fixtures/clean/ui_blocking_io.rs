fn tick() {
    crate::ancillary_io::dispatch(move || {
        std::fs::write("fixture", b"data").map_err(|e| e.to_string())
    });
}

async fn listen() {
    jobs.recv().await;
}

fn offline_reader() {
    // design-lint: allow(ui-blocking-io) Worker-only helper; its caller uses a completion ticket.
    std::fs::read("fixture");
}
