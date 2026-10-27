fn button(studio: &mut Studio) {
    studio.run_command("note.unregistered", args); //~ ops-coverage, ui-blocking-io
}
