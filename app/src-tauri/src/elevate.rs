//! Saving into a folder the user cannot write to, the way native apps do: the file contents are
//! staged in a private temporary folder and copied into place by a command the OS runs after its
//! own administrator prompt (macOS authorization dialog, Windows UAC, Linux polkit).

use choren_core::project::{SavePlan, SaveStep};
use std::path::{Path, PathBuf};

/// Why an elevated save did not happen.
pub enum Failure {
    Cancelled,
    Failed(String),
}

/// Applies `plan` with administrator rights once the user authorizes it.
pub fn apply(plan: &SavePlan) -> Result<(), Failure> {
    let root = plan.dir.canonicalize().map_err(|e| Failure::Failed(format!("Cannot open {}: {e}", plan.dir.display())))?;
    let stage = stage_dir().map_err(|e| Failure::Failed(format!("Cannot prepare the save: {e}")))?;
    let result = (|| {
        let mut ops = vec![];
        for step in &plan.steps {
            ops.push(match step {
                SaveStep::Write(rel, bytes) => {
                    let target = confined(&plan.dir, &root, rel)?;
                    let staged = stage.join(ops.len().to_string());
                    std::fs::write(&staged, bytes).map_err(|e| Failure::Failed(format!("Cannot prepare the save: {e}")))?;
                    Op::Write(staged, target)
                }
                SaveStep::Delete(rel) => Op::Delete(confined(&plan.dir, &root, rel)?),
                SaveStep::RemoveFolder(rel) => Op::RemoveFolder(confined(&plan.dir, &root, rel)?),
            });
        }
        run(&stage, &plan.dir, &ops)
    })();
    let _ = std::fs::remove_dir_all(&stage);
    result
}

enum Op {
    Write(PathBuf, PathBuf),
    Delete(PathBuf),
    RemoveFolder(PathBuf),
}

/// `rel` inside the project folder `dir` (`root` once links are resolved), refusing links that
/// lead out of it: the copy runs with administrator rights.
fn confined(dir: &Path, root: &Path, rel: &str) -> Result<PathBuf, Failure> {
    let target = dir.join(rel);
    let outside = || Failure::Failed(format!("Cannot save {rel}: it links outside the project folder."));
    if target.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(outside());
    }
    let existing = target.ancestors().find(|a| a.exists()).unwrap_or(dir);
    let real = existing.canonicalize().map_err(|e| Failure::Failed(format!("Cannot save {rel}: {e}")))?;
    if !real.starts_with(root) {
        return Err(outside());
    }
    Ok(target)
}

fn stage_dir() -> std::io::Result<PathBuf> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let dir = std::env::temp_dir().join(format!("choren-save-{}-{nanos}", std::process::id()));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&dir)?;
    Ok(dir)
}

/// A POSIX shell script performing `ops`. Existing files are overwritten in place so they keep
/// their owner and permissions.
#[cfg(unix)]
fn shell_script(ops: &[Op]) -> String {
    let q = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"));
    let mut script = String::from("set -e\n");
    for op in ops {
        script += &match op {
            Op::Write(from, to) => format!("[ ! -L {t} ] || exit 3\nmkdir -p {d}\ncp {f} {t}\n", f = q(from), t = q(to), d = q(to.parent().unwrap())),
            Op::Delete(path) => format!("rm -f {}\n", q(path)),
            Op::RemoveFolder(path) => format!("rmdir {} 2>/dev/null || true\n", q(path)),
        };
    }
    script
}

#[cfg(target_os = "macos")]
fn run(_stage: &Path, root: &Path, ops: &[Op]) -> Result<(), Failure> {
    let applescript = |s: &str| s.replace('\\', r"\\").replace('"', "\\\"").replace('\n', r"\n").replace('\r', r"\r");
    let prompt = format!("Choren needs administrator access to save the project in “{}”.", root.display());
    let out = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(format!("do shell script \"{}\" with prompt \"{}\" with administrator privileges", applescript(&shell_script(ops)), applescript(&prompt)))
        .output()
        .map_err(|e| Failure::Failed(format!("Cannot ask for administrator access: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    let error = String::from_utf8_lossy(&out.stderr);
    // -128: the user pressed Cancel.
    if error.contains("(-128)") { Err(Failure::Cancelled) } else { Err(Failure::Failed(format!("Cannot save as administrator: {}", error.trim()))) }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn run(_stage: &Path, _root: &Path, ops: &[Op]) -> Result<(), Failure> {
    let out = std::process::Command::new("pkexec")
        .args(["/bin/sh", "-c", &shell_script(ops)])
        .output()
        .map_err(|e| Failure::Failed(format!("Cannot ask for administrator access (pkexec): {e}")))?;
    match out.status.code() {
        Some(0) => Ok(()),
        // 126: the authentication dialog was dismissed.
        Some(126) => Err(Failure::Cancelled),
        _ => Err(Failure::Failed(format!("Cannot save as administrator: {}", String::from_utf8_lossy(&out.stderr).trim()))),
    }
}

#[cfg(windows)]
fn run(stage: &Path, _root: &Path, ops: &[Op]) -> Result<(), Failure> {
    use std::os::windows::process::CommandExt;
    let arg = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let q = |p: &Path| arg(&p.to_string_lossy());
    let mut script = String::from("$ErrorActionPreference = 'Stop'\n");
    for op in ops {
        script += &match op {
            Op::Write(from, to) => format!("New-Item -ItemType Directory -Force -Path {} | Out-Null\nCopy-Item -LiteralPath {} -Destination {} -Force\n", q(to.parent().unwrap()), q(from), q(to)),
            Op::Delete(path) => format!("if (Test-Path -LiteralPath {p}) {{ Remove-Item -LiteralPath {p} -Force }}\n", p = q(path)),
            Op::RemoveFolder(path) => format!("try {{ [IO.Directory]::Delete({}) }} catch {{}}\n", q(path)),
        };
    }
    let (save, launch) = (stage.join("save.ps1"), stage.join("launch.ps1"));
    // Start-Process joins its arguments unquoted, so the script path is quoted inside one argument.
    let launcher = format!(
        "try {{ $p = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList {} }} catch {{ exit 1223 }}\nexit $p.ExitCode\n",
        arg(&format!("-NoProfile -ExecutionPolicy Bypass -File \"{}\"", save.display()))
    );
    // Windows PowerShell reads scripts without a byte order mark as ANSI.
    for (file, text) in [(&save, script), (&launch, launcher)] {
        std::fs::write(file, [b"\xEF\xBB\xBF".as_slice(), text.as_bytes()].concat()).map_err(|e| Failure::Failed(format!("Cannot prepare the save: {e}")))?;
    }
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&launch)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| Failure::Failed(format!("Cannot ask for administrator access: {e}")))?;
    match status.code() {
        Some(0) => Ok(()),
        // ERROR_CANCELLED: the UAC prompt was declined.
        Some(1223) => Err(Failure::Cancelled),
        _ => Err(Failure::Failed("Cannot save as administrator: copying the files failed.".into())),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn shell_script_copies_deletes_and_refuses_links_out() {
        let root = stage_dir().unwrap().canonicalize().unwrap();
        let odd = "a b/it's \"q\"\n$(x).xar";
        std::fs::write(root.join("old.txt"), "old").unwrap();
        std::fs::create_dir(root.join("gone")).unwrap();
        std::fs::write(root.join("gone/x"), "x").unwrap();
        std::fs::write(root.join("keep.txt"), "before").unwrap();
        let plan = SavePlan { dir: root.clone(), steps: vec![
            SaveStep::Write(odd.into(), b"new".to_vec()),
            SaveStep::Write("keep.txt".into(), b"after".to_vec()),
            SaveStep::Delete("gone/x".into()),
            SaveStep::RemoveFolder("gone".into()),
        ] };
        let stage = stage_dir().unwrap();
        let ops: Vec<Op> = plan.steps.iter().enumerate().map(|(i, s)| match s {
            SaveStep::Write(rel, bytes) => { std::fs::write(stage.join(i.to_string()), bytes).unwrap(); Op::Write(stage.join(i.to_string()), confined(&root, &root, rel).ok().unwrap()) }
            SaveStep::Delete(rel) => Op::Delete(confined(&root, &root, rel).ok().unwrap()),
            SaveStep::RemoveFolder(rel) => Op::RemoveFolder(confined(&root, &root, rel).ok().unwrap()),
        }).collect();
        let status = std::process::Command::new("/bin/sh").args(["-c", &shell_script(&ops)]).status().unwrap();
        assert!(status.success());
        assert_eq!(std::fs::read(root.join(odd)).unwrap(), b"new");
        assert_eq!(std::fs::read(root.join("keep.txt")).unwrap(), b"after");
        assert!(!root.join("gone").exists());

        std::os::unix::fs::symlink("/etc", root.join("out")).unwrap();
        std::os::unix::fs::symlink("/etc/hosts", root.join("hosts")).unwrap();
        assert!(confined(&root, &root, "out/x").is_err());
        assert!(confined(&root, &root, "hosts").is_err());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&stage);
    }
}
