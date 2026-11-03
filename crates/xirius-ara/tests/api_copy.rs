//! The copied ARA API headers are the ones `third_party/ara_api/SHA256SUMS` lists (taken from
//! the commits its README names), so a copy changed by hand or by line endings shows here.

use sha2::{Digest, Sha256};
use std::path::Path;

#[test]
fn the_copied_headers_are_the_listed_ones() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/ara_api");
    let sums = std::fs::read_to_string(dir.join("SHA256SUMS")).unwrap();
    let mut checked = 0;
    for line in sums.lines().filter(|l| !l.trim().is_empty()) {
        let (want, name) = line.split_once("  ").expect("<hash>  <file>");
        let bytes = std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let got: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(got, want, "{name} differs from the copy listed");
        checked += 1;
    }
    assert_eq!(checked, 6);
}
