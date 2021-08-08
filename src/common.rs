use anyhow::{Result};
use std::fs;
use std::fs::{File};
use std::path::{Path, PathBuf};
use std::io;
use std::io::{Read};

#[cfg(not(windows))]
pub const DEVNULL: &str = "/dev/null";

#[cfg(windows)]
pub const DEVNULL: &str = "nul";

pub fn copy_tempfile(name: &Path) -> Result<(PathBuf, File)> {
    let tempname: PathBuf = [name, Path::new("XXXXXX")].iter().collect();
    let file = File::create(&tempname)?;
    let statbuf = fs::metadata(name)?.permissions();
    fs::set_permissions(&tempname, statbuf)?;
    Ok((tempname, file))
}

pub struct Input {
    file: Option<File>
}

impl Input {
    pub fn new(f: Option<&Path>) -> Result<Self> {
        match f {
            Some(v) => Ok(Input {
                file: Some(File::open(v)?)
            }),
            None => Ok(Input {
                file: None
            })
        }
    }
}

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.file.as_mut() {
            Some(v) => v.read(buf),
            None => io::stdin().read(buf)
        }
    }
}
