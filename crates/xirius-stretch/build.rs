fn main() {
    println!("cargo::rerun-if-changed=src/wrapper.cpp");
    println!("cargo::rerun-if-changed=vendor");

    cc::Build::new()
        .file("src/wrapper.cpp")
        .include("vendor")
        .cpp(true)
        .std("c++14")
        // MSVC は BOM の無いソースを cp932 で読み、日本語のコメントが次の行を飲む。
        .flag_if_supported("/utf-8")
        .flag_if_supported("/EHsc")
        // 最適化しないと、開発版で 1 分の音に何十秒もかかる。
        .opt_level(2)
        // 32 bit の size の変換の警告は、同梱のヘッダーのもの。
        .warnings(false)
        .compile("xirius-stretch");
}
