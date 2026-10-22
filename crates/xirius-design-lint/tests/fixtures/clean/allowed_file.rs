// design-lint: allow-file(raw-color) 外部の表の写し。ファイル全体の例外

const TABLE: [u32; 2] = [0x336699, 0x112233];

fn colors() {
    let a = rgb(0x336699);
    let b = "#112233";
}
