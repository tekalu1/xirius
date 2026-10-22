// The version information of the Windows programs (LIC-022): what Explorer's 「詳細」 and the
// Task Manager show for each exe (its name, version, publisher and copyright), and the icon for
// the one with a window. The build script of each crate that makes a program shipped with the
// app includes this file (`include!("../../packaging/windows/version_info.rs")`), so the
// publisher and the copyright are written here only. The version is the crate's, which is the
// workspace's (`version.workspace = true`).

/// The publisher, as the copyright line of LICENSE-MIT names it.
#[allow(dead_code)]
const AUTHORS: &str = "The Xirius Authors";

/// The copyright line of LICENSE-MIT.
#[allow(dead_code)]
const COPYRIGHT: &str = "Copyright (c) 2026 The Xirius Authors";

#[allow(dead_code)]
const PRODUCT: &str = "Xirius";

/// Puts the version information into each program of `bins` (its name without `.exe`, and what
/// it is, in a few words of ASCII), and `icon` (a path from the crate's folder) as the first
/// icon, which is also the one GPUI gives the window.
#[allow(dead_code)]
fn version_info(bins: &[(&str, &str)], icon: Option<&str>) {
    println!("cargo:rerun-if-changed=../../packaging/windows/version_info.rs");
    if let Some(icon) = icon {
        println!("cargo:rerun-if-changed={icon}");
    }
    #[cfg(windows)]
    {
        let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let version = std::env::var("CARGO_PKG_VERSION").unwrap();
        let icon = icon.map(|i| {
            let path = dir.join(i).canonicalize().expect("the icon is there");
            // rc.exe reads `\` in a string as an escape; `/` is a separator to it as well.
            let path = path.to_string_lossy().replace('\\', "/");
            path.strip_prefix("//?/").unwrap_or(&path).to_string()
        });
        for (bin, description) in bins {
            let rc = out.join(format!("{bin}-version.rc"));
            std::fs::write(&rc, resource(bin, description, &version, icon.as_deref())).unwrap();
            embed_resource::compile_for(&rc, [bin], embed_resource::NONE)
                .manifest_optional()
                .expect("could not embed the version information");
        }
    }
}

/// The resource script of `bin`: the version information as resource 1, and the icon.
#[allow(dead_code)]
fn resource(bin: &str, description: &str, version: &str, icon: Option<&str>) -> String {
    // Windows keeps four numbers of up to 65535: the pre-release and build parts are dropped,
    // and the file is marked as a pre-release instead.
    let core = version.split(['-', '+']).next().unwrap();
    let mut numbers: Vec<u16> = core.split('.').map(|n| n.parse().unwrap()).collect();
    numbers.resize(4, 0);
    let commas = numbers.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
    let dots = commas.replace(',', ".");
    let flags = if version.contains('-') { "0x2" } else { "0x0" };
    let s = |v: &str| format!("\"{}\"", v.replace('"', "\"\""));
    let mut rc = String::new();
    if let Some(icon) = icon {
        rc += &format!("1 ICON {}\n", s(icon));
    }
    rc += &format!(
        "1 VERSIONINFO\n\
         FILEVERSION {commas}\n\
         PRODUCTVERSION {commas}\n\
         FILEFLAGSMASK 0x3F\n\
         FILEFLAGS {flags}\n\
         FILEOS 0x40004\n\
         FILETYPE 0x1\n\
         FILESUBTYPE 0x0\n\
         BEGIN\n\
         BLOCK \"StringFileInfo\"\n\
         BEGIN\n\
         BLOCK \"040904B0\"\n\
         BEGIN\n\
         VALUE \"CompanyName\", {company}\n\
         VALUE \"FileDescription\", {description}\n\
         VALUE \"FileVersion\", {dots}\n\
         VALUE \"InternalName\", {bin_name}\n\
         VALUE \"LegalCopyright\", {copyright}\n\
         VALUE \"OriginalFilename\", {file}\n\
         VALUE \"ProductName\", {product}\n\
         VALUE \"ProductVersion\", {version}\n\
         END\n\
         END\n\
         BLOCK \"VarFileInfo\"\n\
         BEGIN\n\
         VALUE \"Translation\", 0x409, 1200\n\
         END\n\
         END\n",
        company = s(AUTHORS),
        description = s(description),
        dots = s(&dots),
        bin_name = s(bin),
        copyright = s(COPYRIGHT),
        file = s(&format!("{bin}.exe")),
        product = s(PRODUCT),
        version = s(version),
    );
    rc
}
