# Security policy

## Reporting a vulnerability

Please do not open a public issue for a security problem.

Report it privately through GitHub: on the repository page, open **Security → Report a
vulnerability** (GitHub's private vulnerability reporting). Include the version (the app
menu's 「Xirius について」 (About Xirius), or `xirius --version`), the operating system,
and the steps or the file that shows the problem.

You should get an answer within 7 days. A fix is released as a new version on the
releases page; the report is published as a GitHub security advisory once the fix is out,
crediting you unless you ask not to be named.

## Supported versions

Only the latest release gets security fixes.

## What counts

Xirius is a desktop application that opens files and loads plugins. Things we treat as
security problems include:

- A song, project (`.dawproject`), audio or MIDI file that runs code or writes outside
  the folders it should when it is opened.
- The MCP server (`xirius mcp --http`) listening on an address other than `127.0.0.1` /
  `::1`, or answering a request without its token.
- The update check (one request a day to the GitHub releases API, off with
  `app.update.check`) sending anything other than that request, or trusting what it
  should not.
- A downloaded release whose checksum does not match `SHA256SUMS.txt` on the release
  page.

Not security problems by themselves:

- A plugin (VST3 or CLAP) doing harm. A plugin is a program you chose to run; Xirius
  loads it with the same rights as the application. Plugins can be run in a separate
  process so that a crash does not take the song with it, but that is not a sandbox.
- The Windows SmartScreen warning on the unsigned builds (see "Installing on Windows" in
  `README.md`).
