# The icons of the app

The SVG files here are the icons of Xirius's screens: the bars, the tools, the tracks, the
transport, the mixer, the browser and the rest.

## Where they come from

They were drawn for Xirius (from the approved mockups in `docs/ui-approved.md`), by the Xirius
authors, and are part of the repository under the project's licence:
**MIT OR Apache-2.0** (see `LICENSE-MIT` and `LICENSE-APACHE` at the top of the repository).
They are not taken from an icon set: they carry no other set's marks or licence lines, and they
follow the app's own grid and stroke (24 × 24, `currentColor`, 1.6 wide, round ends).

The GitHub mark used in 「Xirius について」 is **not** in this folder: it is `mark-github-16.svg`
in `crates/xirius-ui/assets/octicons`, from Octicons (Copyright (c) GitHub Inc., MIT), with its
licence text beside it and its line in `NOTICE`.

## Adding an icon

- Draw it on the same 24 × 24 grid, with `fill="none"`, `stroke="currentColor"`,
  `stroke-width="1.6"`, round ends and joins, so the colour comes from the screen and it sits
  with the others.
- Name it for what it shows (`record.svg`), and add it to the design lint's list if the lint
  asks for it.
- If it comes from somewhere else, say where and under which licence in this file, and add the
  licence text and the line in `NOTICE` and `THIRD-PARTY-LICENSES.txt`.
