//! Hashes the solver source into `SOLVER_DIGEST`, so a snapshot can say which source it was
//! measured against.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");

    let mut files = Vec::new();
    collect(Path::new("src"), &mut files)?;
    files.push(PathBuf::from("Cargo.toml"));
    // Forward slashes keep the digest the same on every platform.
    let mut files: Vec<(String, PathBuf)> = files
        .into_iter()
        .map(|p| (p.to_string_lossy().replace('\\', "/"), p))
        .collect();
    files.sort();

    let mut hasher = Sha256::new();
    for (name, path) in &files {
        hasher.update(name.as_bytes());
        hasher.update([0]);
        // Line endings are normalized so a Windows checkout hashes like a Unix one.
        let text = fs::read(path)?;
        let text: Vec<u8> = text.into_iter().filter(|&b| b != b'\r').collect();
        hasher.update((text.len() as u64).to_le_bytes());
        hasher.update(&text);
    }
    let digest: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    println!("cargo:rustc-env=LEX_SOLVER_DIGEST={digest}");
    Ok(())
}
