use anyhow::{Result};
use std::fs;
use std::fs::{File};
use std::path::{Path, PathBuf};
use std::io::{BufRead, BufReader, Lines};

#[cfg(not(windows))]
pub const devnull: &str = "/dev/null";

#[cfg(windows)]
pub const devnull: &str = "nul";

pub fn read_lines(file: File) -> Result<Lines<BufReader<File>>> {
    Ok(BufReader::new(file).lines())
}

pub fn copy_tempfile(file: &File, name: &Path) -> Result<(PathBuf, File)> {
    let tempname: PathBuf = [name, Path::new("XXXXXX")].iter().collect();
    let file = File::create(tempname)?;
    let statbuf = fs::metadata(name)?.permissions();
    fs::set_permissions(tempname, statbuf)?;
    Ok((tempname, file))
}
