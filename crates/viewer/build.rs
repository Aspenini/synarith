//! Compile only the host platform's small widget adapter.
fn main() {
    println!("cargo:rerun-if-changed=native");
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let mut build = cc::Build::new();
    build.cpp(true).include("native").std("c++17");
    match target.as_str() {
        "windows" => {
            build.file("native/windows.cpp");
            if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
                build.flag("/utf-8");
                println!("cargo:rustc-link-arg-bin=syar-view=/MANIFEST:EMBED");
            }
            for library in ["user32", "gdi32", "comdlg32", "comctl32"] {
                println!("cargo:rustc-link-lib={library}");
            }
        }
        "macos" => {
            build.file("native/macos.mm").flag("-fobjc-arc");
            println!("cargo:rustc-link-lib=framework=AppKit");
            println!("cargo:rustc-link-lib=framework=CoreGraphics");
        }
        "linux" => configure_qt(&mut build),
        _ => panic!("The native viewer supports Windows, macOS, and Linux."),
    }
    build.compile("syar_widgets");
}

#[cfg(target_os = "linux")]
fn configure_qt(build: &mut cc::Build) {
    let mut qt = pkg_config::Config::new()
        .atleast_version("6.2")
        .statik(false)
        .cargo_metadata(false)
        .probe("Qt6Widgets")
        .expect("Install system Qt6 Widgets development libraries and pkg-config (Debian/Ubuntu: qt6-base-dev pkg-config). Qt is never downloaded or bundled.");
    // pkg-config may omit -L for standard system directories. Read the declared
    // libdir as well so shared-library checks work with distro Qt packages.
    let libdir = pkg_config::get_variable("Qt6Widgets", "libdir")
        .expect("The system Qt6Widgets.pc must declare libdir.");
    let libdir = std::path::PathBuf::from(libdir);
    if !qt.link_paths.contains(&libdir) {
        qt.link_paths.push(libdir);
    }
    for library in ["Qt6Widgets", "Qt6Gui", "Qt6Core"] {
        assert!(
            qt.link_paths
                .iter()
                .any(|p| p.join(format!("lib{library}.so")).is_file()
                    || p.join(format!("lib{library}.so.6")).is_file()),
            "Shared system {library} is required; static Qt is unsupported."
        );
    }
    for path in qt.include_paths {
        build.include(path);
    }
    for (name, value) in qt.defines {
        build.define(&name, value.as_deref());
    }
    for path in qt.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
    for library in qt.libs {
        println!("cargo:rustc-link-lib=dylib={library}");
    }
    build.file("native/linux.cpp").flag("-fPIC");
}

#[cfg(not(target_os = "linux"))]
fn configure_qt(_: &mut cc::Build) {
    panic!("Cross-compiling the Qt viewer requires running Cargo on Linux.");
}
