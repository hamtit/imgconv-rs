//! Builds the pinned libaom and libjxl (see scripts/fetch-deps.sh) as static
//! libraries, generates Rust bindings from their headers, and runs the
//! napi-rs setup.
//!
//! For wasm32-unknown-emscripten (scripts/build-wasm.sh) the same sources are
//! built through Emscripten's CMake toolchain without SIMD, assembly or
//! threads; the Emscripten SDK must be on PATH (or EMSCRIPTEN_ROOT set).

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn emscripten() -> bool {
    env::var("TARGET").is_ok_and(|t| t.contains("emscripten"))
}

/// Emscripten's own directory (holds the CMake toolchain file).
fn emscripten_root() -> PathBuf {
    println!("cargo:rerun-if-env-changed=EMSCRIPTEN_ROOT");
    if let Some(root) = env::var_os("EMSCRIPTEN_ROOT") {
        return PathBuf::from(root);
    }
    let out = Command::new("em-config")
        .arg("EMSCRIPTEN_ROOT")
        .output()
        .expect("em-config not found: activate the Emscripten SDK or set EMSCRIPTEN_ROOT");
    assert!(out.status.success(), "em-config EMSCRIPTEN_ROOT failed");
    PathBuf::from(String::from_utf8(out.stdout).unwrap().trim())
}

/// clang arguments for bindgen: wasm32 type sizes and the include directories
/// emcc itself searches (its libc headers are not in a plain --sysroot, so
/// ask the compiler instead of guessing the layout).
fn emscripten_clang_args() -> Vec<String> {
    let out = Command::new("emcc")
        .args(["-E", "-x", "c", "-v", "-"])
        .stdin(std::process::Stdio::null())
        .output()
        .expect("emcc not found: activate the Emscripten SDK");
    let log = String::from_utf8_lossy(&out.stderr);
    let mut args = vec!["--target=wasm32-unknown-emscripten".to_string()];
    let mut in_list = false;
    for line in log.lines() {
        if line.starts_with("#include <...> search starts here") {
            in_list = true;
        } else if line.starts_with("End of search list") {
            break;
        } else if in_list {
            args.push("-isystem".to_string());
            args.push(line.trim().to_string());
        }
    }
    assert!(args.len() > 1, "could not read emcc's include paths:\n{log}");
    args
}

/// TEMPORARY: report what emcc itself makes of the header, as cargo warnings.
fn diagnose_emscripten(header: &str, include: &std::path::Path) {
    let dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let file = dir.join("diag.h");
    std::fs::write(&file, header).unwrap();
    println!("cargo:warning=diag include={} args={:?}", include.display(), emscripten_clang_args());
    println!("cargo:warning=diag bindgen clang {}", bindgen::clang_version().full);
    // The same libclang family bindgen uses, with and without emcc's own
    // (newer) clang resource directory.
    for (label, skip_resource) in [("with-resource", false), ("without-resource", true)] {
        let mut cmd = Command::new("clang");
        let args = emscripten_clang_args();
        let mut it = args.iter().peekable();
        while let Some(a) = it.next() {
            if a == "-isystem" {
                let p = it.next().unwrap();
                if skip_resource && p.contains("lib/clang") {
                    continue;
                }
                cmd.arg("-isystem").arg(p);
            } else {
                cmd.arg(a);
            }
        }
        let out = cmd.arg("-fsyntax-only").arg(format!("-I{}", include.display())).arg(&file).output().unwrap();
        println!("cargo:warning=diag {label}: status {}", out.status);
        for l in String::from_utf8_lossy(&out.stderr).lines().filter(|l| l.contains("error")).take(15) {
            println!("cargo:warning=diag {label}: {l}");
        }
    }
}

/// Point a CMake build at the Emscripten toolchain (what `emcmake` does).
fn use_emscripten(cfg: &mut cmake::Config) {
    let root = emscripten_root();
    cfg.define("CMAKE_TOOLCHAIN_FILE", root.join("cmake/Modules/Platform/Emscripten.cmake"));
}

fn main() {
    if env::var_os("CARGO_FEATURE_NAPI").is_some() {
        napi_build::setup();
    }
    build_aom();
    build_jxl();
}

fn vendor_dir(name: &str, env_var: &str) -> PathBuf {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let src = env::var_os(env_var).map(PathBuf::from).unwrap_or_else(|| manifest_dir.join("vendor").join(name));
    println!("cargo:rerun-if-env-changed={env_var}");
    println!("cargo:rerun-if-changed={}", src.join(".version").display());
    if !src.join("CMakeLists.txt").exists() {
        panic!("{name} source not found at {}. Run scripts/fetch-deps.sh first.", src.display());
    }
    src
}

fn bindings(header: &str, include: &std::path::Path, prefix: &str, out: &str) {
    let mut builder = bindgen::Builder::default();
    if emscripten() {
        builder = builder.clang_args(emscripten_clang_args());
        diagnose_emscripten(header, include);
    }
    builder
        .header_contents("wrap.h", header)
        .clang_arg(format!("-I{}", include.display()))
        .allowlist_function(format!("{prefix}.*"))
        .allowlist_type(format!("{prefix}.*"))
        .allowlist_var(format!("{}.*", prefix.to_uppercase()))
        .default_enum_style(bindgen::EnumVariation::Consts)
        .layout_tests(false)
        .generate()
        .unwrap_or_else(|e| panic!("generate {out}: {e}"))
        .write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join(out))
        .unwrap();
    if emscripten() {
        report_bindings(out);
    }
}

/// TEMPORARY: summarize a generated bindings file as cargo warnings.
fn report_bindings(out: &str) {
    let text = std::fs::read_to_string(PathBuf::from(env::var("OUT_DIR").unwrap()).join(out)).unwrap();
    let fns = text.matches("pub fn ").count();
    println!("cargo:warning=diag {out}: {} bytes, {fns} functions", text.len());
    for l in text.lines().filter(|l| l.contains("extern") || l.contains("aom_codec_version_str") || l.contains("JxlEncoderCreate")).take(8) {
        println!("cargo:warning=diag {out}: {l}");
    }
}

fn build_jxl() {
    let src = vendor_dir("libjxl", "IMGCONV_JXL_SOURCE_DIR");
    let mut cfg = cmake::Config::new(&src);
    cfg.profile("Release")
        .define("BUILD_TESTING", "OFF")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("JPEGXL_ENABLE_TOOLS", "OFF")
        .define("JPEGXL_ENABLE_DOXYGEN", "OFF")
        .define("JPEGXL_ENABLE_MANPAGES", "OFF")
        .define("JPEGXL_ENABLE_BENCHMARK", "OFF")
        .define("JPEGXL_ENABLE_EXAMPLES", "OFF")
        .define("JPEGXL_ENABLE_JNI", "OFF")
        .define("JPEGXL_ENABLE_SJPEG", "OFF")
        .define("JPEGXL_ENABLE_OPENEXR", "OFF")
        .define("JPEGXL_ENABLE_JPEGLI", "OFF")
        .define("JPEGXL_ENABLE_TCMALLOC", "OFF")
        .define("JPEGXL_BUNDLE_LIBPNG", "OFF")
        .define("JPEGXL_FORCE_SYSTEM_BROTLI", "OFF")
        .define("JPEGXL_FORCE_SYSTEM_HWY", "OFF")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("CMAKE_INSTALL_LIBDIR", "lib");
    if emscripten() {
        use_emscripten(&mut cfg);
        cfg.define("JPEGXL_ENABLE_WASM_THREADS", "OFF");
    }
    let out = cfg.build();
    println!("cargo:rustc-link-search=native={}", out.join("lib").display());
    // libjxl_threads is not built for wasm (no threads).
    let libs: &[&str] = if emscripten() {
        &["jxl", "jxl_cms", "hwy", "brotlienc", "brotlidec", "brotlicommon"]
    } else {
        &["jxl", "jxl_cms", "jxl_threads", "hwy", "brotlienc", "brotlidec", "brotlicommon"]
    };
    for lib in libs {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    // The wasm build is linked by em++, which brings its own libc++.
    if !emscripten() {
        println!("cargo:rustc-link-lib=stdc++");
    }
    let header = if emscripten() {
        "#include <jxl/encode.h>\n#include <jxl/decode.h>\n"
    } else {
        "#include <jxl/encode.h>\n#include <jxl/decode.h>\n#include <jxl/thread_parallel_runner.h>\n"
    };
    bindings(
        header,
        &out.join("include"),
        "Jxl",
        "jxl_bindings.rs",
    );
}

fn build_aom() {
    let src = vendor_dir("libaom", "IMGCONV_AOM_SOURCE_DIR");

    let target = env::var("TARGET").unwrap();
    let mut cfg = cmake::Config::new(&src);
    cfg.profile("Release")
        .define("ENABLE_DOCS", "0")
        .define("ENABLE_EXAMPLES", "0")
        .define("ENABLE_TESTDATA", "0")
        .define("ENABLE_TESTS", "0")
        .define("ENABLE_TOOLS", "0")
        .define("ENABLE_NASM", "1")
        .define("CONFIG_AV1_ENCODER", "1")
        .define("CONFIG_AV1_DECODER", "1")
        .define("CONFIG_PIC", "1")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        .define("CMAKE_INSTALL_LIBDIR", "lib");
    if emscripten() {
        use_emscripten(&mut cfg);
        cfg.define("AOM_TARGET_CPU", "generic")
            .define("ENABLE_NASM", "0")
            .define("CONFIG_RUNTIME_CPU_DETECT", "0")
            .define("CONFIG_MULTITHREAD", "0")
            .define("CONFIG_WEBM_IO", "0");
    } else if target.starts_with("aarch64") && env::var("HOST").unwrap() != target {
        cfg.define("CMAKE_SYSTEM_PROCESSOR", "aarch64")
            .define("CMAKE_SYSTEM_NAME", "Linux");
    }
    let out = cfg.build();

    println!("cargo:rustc-link-search=native={}", out.join("lib").display());
    println!("cargo:rustc-link-lib=static=aom");

    let include = out.join("include");
    let mut builder = bindgen::Builder::default();
    if emscripten() {
        builder = builder.clang_args(emscripten_clang_args());
        diagnose_emscripten("#include <aom/aom_encoder.h>\n#include <aom/aomcx.h>\n#include <aom/aom_decoder.h>\n#include <aom/aomdx.h>\n", &include);
    }
    let bindings = builder
        .header_contents(
            "aom_wrap.h",
            "#include <aom/aom_encoder.h>\n#include <aom/aomcx.h>\n#include <aom/aom_decoder.h>\n#include <aom/aomdx.h>\n",
        )
        .clang_arg(format!("-I{}", include.display()))
        .allowlist_function("aom_.*")
        .allowlist_type("aom_.*|aome_.*|aomd_.*|av1_.*|aom_codec_control_type_.*")
        .allowlist_var("AOM_.*|AV1E_.*|AOME_.*|AV1_.*")
        .default_enum_style(bindgen::EnumVariation::Consts)
        .layout_tests(false)
        .generate()
        .expect("generate libaom bindings");
    bindings
        .write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join("aom_bindings.rs"))
        .unwrap();
    if emscripten() {
        report_bindings("aom_bindings.rs");
    }
}
