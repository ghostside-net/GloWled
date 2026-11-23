use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    slint_build::compile("ui/settings.slint").expect("Failed to compile Slint UI");

    println!("cargo:rerun-if-changed=icon.ico");
    println!("cargo:rerun-if-changed=assets/glowled.manifest");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_resources();
    }
}

fn parse_version_quad(version: &str) -> (u16, u16, u16, u16) {
    let base = version.split(['-', '+']).next().unwrap_or("0.0.0");
    let mut parts = base.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch, 0)
}

fn rc_escape_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn try_run(cmd: &mut Command) -> bool {
    matches!(cmd.status(), Ok(status) if status.success())
}

fn embed_windows_resources() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();

    let icon_path = manifest_dir.join("icon.ico");
    let manifest_path = manifest_dir.join("assets").join("glowled.manifest");
    assert!(
        icon_path.exists(),
        "Missing Windows icon resource at {}",
        icon_path.display()
    );
    assert!(
        manifest_path.exists(),
        "Missing Windows manifest resource at {}",
        manifest_path.display()
    );

    let pkg_version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "2.0.0".to_string());
    let (maj, min, pat, rev) = parse_version_quad(&pkg_version);
    let rc_content = format!(
        r#"1 ICON "{icon}"
1 24 "{manifest}"

1 VERSIONINFO
FILEVERSION {maj},{min},{pat},{rev}
PRODUCTVERSION {maj},{min},{pat},{rev}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "GloWled - High-Performance WLED Ambilight Controller"
      VALUE "FileVersion", "{pkg_version}"
      VALUE "InternalName", "glowled"
      VALUE "LegalCopyright", "GPL-3.0-only"
      VALUE "OriginalFilename", "glowled.exe"
      VALUE "ProductName", "GloWled"
      VALUE "ProductVersion", "{pkg_version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = rc_escape_path(&icon_path),
        manifest = rc_escape_path(&manifest_path),
    );

    let rc_path = out_dir.join("glowled.rc");
    fs::write(&rc_path, rc_content).expect("Failed to write Windows .rc file");

    let res_path = out_dir.join("glowled.res");
    let auto_inc = if target_env == "gnu" { "gnu" } else { "any" };

    // 1. Try `zig rc` (works cross-platform on Linux and Windows without external SDK tools)
    // Pass `--` before paths so Unix paths starting with `/` are not parsed as `/h` flags.
    let mut zig_cmd = Command::new("zig");
    zig_cmd
        .arg("rc")
        .arg("/:auto-includes")
        .arg(auto_inc)
        .arg("--")
        .arg(&rc_path)
        .arg(&res_path);
    if try_run(&mut zig_cmd) && res_path.exists() {
        println!("cargo:rustc-link-arg-bin=glowled={}", res_path.display());
        return;
    }

    // 2. Native MSVC fallback (`rc.exe` or `llvm-rc`)
    if target_env == "msvc" {
        for tool in ["rc", "llvm-rc"] {
            let mut cmd = Command::new(tool);
            cmd.arg("/fo").arg(&res_path).arg(&rc_path);
            if try_run(&mut cmd) && res_path.exists() {
                println!("cargo:rustc-link-arg-bin=glowled={}", res_path.display());
                return;
            }
        }
    }

    // 3. MinGW/GNU windres fallback
    if target_env == "gnu" {
        let obj_path = out_dir.join("glowled_res.o");
        for tool in ["x86_64-w64-mingw32-windres", "windres"] {
            let mut cmd = Command::new(tool);
            cmd.arg("-i")
                .arg(&rc_path)
                .arg("-O")
                .arg("coff")
                .arg("-o")
                .arg(&obj_path);
            if try_run(&mut cmd) && obj_path.exists() {
                println!("cargo:rustc-link-arg-bin=glowled={}", obj_path.display());
                return;
            }
        }
    }

    panic!(
        "Failed to compile Windows resources ({}) for target_env='{target_env}'. Ensure `zig`, `rc`/`llvm-rc`, or `windres` is installed.",
        rc_path.display()
    );
}
