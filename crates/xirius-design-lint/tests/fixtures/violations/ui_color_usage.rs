fn words(pen: &mut Pen, c: &Colors, r: Rect) {
    let light = c.ink.opacity(0.4);
    pen.text("text", size, weight, light, r, align); //~ text-opacity
    pen.round(r, c.fill.opacity(0.2), radius);
    pen.digits("32", size, weight, c.ink, r, align); //~ translucent-text-surface
    pen.text("text", size, weight, c.surface_3, other, align); //~ color-family
    div().bg(c.ink); //~ color-family
    pen.round(other, c.tracks[0], radius); //~ exception-family
}
