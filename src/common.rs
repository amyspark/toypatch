use anyhow::{Result};
use std::cmp::{Ordering};
use std::fs;
use std::fs::{File};
use std::path::{Path, PathBuf};
use std::io;
use std::io::{Read};

pub fn DEVNULL() -> &'static Path {
    #[cfg(not(windows))]
    return Path::new("/dev/null");

    #[cfg(windows)]
    return Path::new("nul");
}

/// Open a temporary file to copy an existing file into.
pub fn copy_tempfile(name: &Path) -> Result<(PathBuf, File)> {
    let tempname: PathBuf = [name, Path::new("XXXXXX")].iter().collect();
    let file = File::create(&tempname)?;
    let statbuf = fs::metadata(name)?.permissions();
    fs::set_permissions(&tempname, statbuf)?;
    Ok((tempname, file))
}

/// Compare ignoring whitespace. Just returns 0/1, no > or <
pub fn loosecmp(aa: &str, bb: &str) -> Ordering {
    let mut aa = aa.chars().peekable();
    let mut bb = bb.chars().peekable();

    loop {
        aa.by_ref().skip_while(|c| c.is_ascii_whitespace());
        bb.by_ref().skip_while(|c| c.is_ascii_whitespace());
        if aa.peek() != bb.peek() {
            return Ordering::Greater;
        }
        if aa.peek() == None {
            return Ordering::Equal;
        }
        aa.next();
        bb.next();
    }
}

#[derive(Debug, Default)]
pub struct Input {
    file: Option<File>
}

// impl<'a> Input<'a> {
//     pub fn new(f: Option<&Path>) -> Result<Self> {
//         match f {
//             Some(v) => Ok(Input::from(&File::open(v)?)),
//             None => Ok(Input {
//                 file: None
//             })
//         }
//     }
// }

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.file.as_mut() {
            Some(v) => v.read(buf),
            None => io::stdin().read(buf)
        }
    }
}

impl From<File> for Input {
    fn from(f: File) -> Self {
        Input{
            file: Some(f)
        }
    }
}

impl From<Option<File>> for Input {
    fn from(f: Option<File>) -> Self {
        Input { file: f }
    }
}
