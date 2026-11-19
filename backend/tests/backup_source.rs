// SPDX-License-Identifier: AGPL-3.0-only
use anyhow::Result;
use std::{fs, process::Command};
use yappers_netkit::store::Store;

#[test]
fn backup_requires_an_existing_initialized_home() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let source = dir.path().join("home.db");
    let output = dir.path().join("archive");
    let backup = || {
        Command::new(env!("CARGO_BIN_EXE_yappers-server"))
            .arg("backup")
            .arg("--database")
            .arg(&source)
            .arg("--output")
            .arg(&output)
            .output()
    };
    assert!(!backup()?.status.success());
    assert_eq!(fs::read_dir(dir.path())?.count(), 0);
    fs::write(&source, [])?;
    assert!(!backup()?.status.success());
    assert!(fs::read(&source)?.is_empty());
    assert!(!output.exists());
    assert!(!source.with_added_extension("deletion-guard").exists());
    Store::open(&source)?;
    assert!(backup()?.status.success());
    let restored = dir.path().join("restored.db");
    yappers_netkit::backup::restore(
        &source.with_added_extension("deletion-guard"),
        &output,
        &restored,
    )?;
    assert!(Store::open(&restored)?.events("community")?.is_empty());
    Ok(())
}
