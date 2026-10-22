fn words(pen: &mut Pen, c: &Colors, r: Rect) {
    pen.round(r, c.surface_3, radius);
    pen.text("text", size, weight, c.ink, r, align);
    pen.round(dot, c.ring.opacity(0.2), radius);
}
