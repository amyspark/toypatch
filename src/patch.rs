mod common;

use crate::common::*;
use clap::Parser;
use anyhow::{anyhow, Result};
use log::debug;
use peeking_take_while::PeekableExt;
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::env;
use std::fs;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// Apply a unified diff to one or more files.
///
/// This version of patch only handles unified diffs, and only modifies
/// a file when all hunks to that file apply. Patch prints failed hunks
/// to stderr, and exits with nonzero status if any hunks fail.
///
/// A file compared against `/dev/null` (or with a date <= the epoch) is
/// created/deleted as appropriate.
#[derive(Default, Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct PatchToy {
    /// Modify files in `dir`
    #[clap(short)]
    dir: Option<PathBuf>,

    /// Input patch file (default = stdin)
    #[clap(short)]
    input: Option<PathBuf>,

    // Loose match (ignore whitespace)
    #[clap(short)]
    loose: Option<bool>,

    /// Number of '/' to strip from start of file paths (default = all)
    #[clap(short = 'p')]
    strip: Option<usize>,

    /// Reverse patch
    #[clap(short = 'R')]
    reverse: bool,

    /// Fuzz
    #[clap(short = 'F')]
    fuzz: Option<usize>,

    /// Silent except for errors
    #[clap(short)]
    silent: bool,

    /// Ignored (only handles "unified" diffs)
    #[clap(short)]
    _unified: bool,

    /// Don't change files, just confirm patch applies
    #[clap(long)]
    dry_run: bool,

    /// Pairs of file and patch to apply.
    #[clap(parse(from_os_str))]
    files: Vec<PathBuf>,
}

#[derive(Default, Debug)]
struct Globals<'a> {
    i: Option<&'a PathBuf>,
    d: Option<&'a str>,

    p: usize,
    g: usize,
    f: usize,

    current_hunk: VecDeque<String>,
    oldline: usize,
    oldlen: usize,
    newline: usize,
    newlen: usize,
    linenum: isize,
    outnum: isize,

    context: usize,
    state: u32,
    filein: Option<File>,
    fileout: Option<File>,
    hunknum: isize,
    tempname: Option<PathBuf>,
    destname: Option<PathBuf>,

    exitval: Option<i32>,
}

/// Dispose of a line of input, either by writing it out or discarding it.
///
/// state < 2: just free
///
/// state = 2: write whole line to stderr
///
/// state = 3: write whole line to fileout
///
/// state > 3: write line+1 to fileout when *line != state
pub fn do_line(outnum: &mut isize, state: &mut u32, fileout: &mut Option<File>, data: &str) -> Result<()> {
    *outnum += 1;
    if *state > 1 {
        if *state == 2 {
            if *state > 3 {
                eprintln!("{}", &data[1..]);
            } else {
                eprintln!("{}", &data[0..]);
            }
        } else {
            let mut f = fileout.as_ref().unwrap();
            if *state > 3 {
                writeln!(f, "{}", &data[1..])?;
            } else {
                writeln!(f, "{}", &data[0..])?;
            }
        }
    }

    debug!("DO {}: {}", state, data);

    Ok(())
}

impl Globals<'_> {
    /// Copy the rest of the data and replace the original with the copy.
    pub fn finish_oldfile(&mut self) -> Result<()> {
        if self.tempname.is_some() {
            if self.filein.is_some() {
                let mut a = self
                    .filein
                    .as_ref()
                    .ok_or_else(|| anyhow!("filein unavailable"))?;
                let mut b = self
                    .fileout
                    .as_ref()
                    .ok_or_else(|| anyhow!("fileout unavailable"))?;
                io::copy(&mut a, &mut b)?;
            }

            fs::rename(
                self.tempname
                    .as_ref()
                    .ok_or_else(|| anyhow!("tempname unset?!"))?,
                self.destname
                    .as_ref()
                    .ok_or_else(|| anyhow!("destname unset?!"))?,
            )?;

            self.tempname = None;
        }

        self.fileout = None;
        self.filein = None;

        Ok(())
    }

    /// TODO: export failed hunk before closing
    pub fn fail_hunk(&mut self, toy: &PatchToy) -> Result<()> {
        if self.current_hunk.is_empty() {
            return Ok(());
        }

        eprintln!(
            "Hunk {} FAILED {}/{}.",
            self.hunknum, self.oldline, self.newline
        );

        self.exitval = Some(1);

        // If we got to this point, we've seeked to the end.  Discard changes to
        // this file and advance to next file.

        self.state = 2;
        self.current_hunk.clear();
        if !toy.dry_run {
            self.filein = None;
            self.fileout = None;
            std::fs::remove_file(
                self.tempname.as_ref()
                    .ok_or_else(|| anyhow!("No temp file to remove"))?,
            )?;
        }
        self.state = 0;

        Ok(())
    }

    /// Given a hunk of a unified diff, make the appropriate change to the file.
    /// This does not use the location information, but instead treats a hunk
    /// as a sort of regex. Copies data from input to output until it finds
    /// the change to be made, then outputs the changed data and returns.
    /// (Finding EOF first is an error.) This is a single pass operation, so
    /// multiple hunks must occur in order in the file.
    pub fn apply_one_hunk(&mut self, toy: &PatchToy) -> Result<u32> {
        // struct double_list *plist, *buf = 0, *check;
        let mut trail = 0;
        let reverse = toy.reverse;
        let mut backwarn = 0;
        let mut allfuzz = 0;
        let mut fuzz = 0;
        let mut i = 0;

        let lcmp = |aa: &str, bb: &str| {
            match toy.loose {
                Some(_) => loosecmp(aa, bb),
                None => aa.cmp(bb)
            }
        };

        // Match EOF if there aren't as many ending context lines as beginning
        {
            fuzz = 0;
            for plist in &self.current_hunk {
                let c = plist;

                match c.starts_with(" ") {
                    true => trail += 1,
                    false => trail = 0,
                }

                // Only allow fuzz if 2 context lines have multiple nonwhitespace chars.
                // avoids the "all context was blank or } lines" issue. Removed lines
                // count as context since they're matched.
                if c.starts_with(" ")
                    || c.starts_with(|d| match reverse {
                        true => d == '+',
                        false => d == '-',
                    })
                {
                    let mut s = plist[1..].chars().skip_while(|c| c.is_ascii_whitespace());
                    
                    match s.nth(1) {
                        Some(v) => {
                            if !v.is_ascii_whitespace() {
                                fuzz += 1;
                            }
                        }
                        None => {}
                    };
                }

                #[cfg(debug_assertions)]
                eprintln!("HUNK:{}", plist);
            }
        }

        let matcheof = trail == 0 || trail < self.context;
        let _allfuzz = match fuzz.cmp(&2) {
            Ordering::Less => 0,
            _ => match toy.fuzz {
                Some(v) => v,
                None => match self.context.cmp(&0) {
                    Ordering::Greater => self.context - 1,
                    _ => 0
                }
            }
        };

        #[cfg(debug_assertions)]
        eprintln!("MATCHEOF={}", matcheof);

        // Loop through input data searching for this hunk. Match all context
        // lines and lines to be removed until we've found end of complete hunk.
        let mut plist = &mut self.current_hunk;
        let mut buf: Vec<String> = vec![];
        let mut check: &[String];
        let mut fuzz = 0;
        let mut filein = match &self.filein {
            Some(v) => BufReader::new(v).lines(),
            None => return Err(anyhow!("Unavailable input!"))
        };

        loop {
            let data = filein.next();

            // Figure out which line of hunk to compare with next. (Skip lines
            // of the hunk we'd be adding.)
            while !plist.is_empty() {
                match plist.front() {
                    Some(v) => {
                        let start = match reverse {
                            true => '-',
                            false => '+'
                        };
                        if v.starts_with(start) {
                            match &data {
                                Some(d) => {
                                    if lcmp(d.as_ref().unwrap(), &v[1..]) == Ordering::Equal {
                                        if backwarn == 0 {
                                            backwarn = self.linenum;
                                        }
                                    }
                                },
                                None => {}
                            }
                        }
                    },
                    None => break
                }
                plist.pop_front();
            }

            // Is this EOF?
            match &data {
                Some(v) => {
                    self.linenum += 1;

                    #[cfg(debug_assertions)]
                    eprintln!("IN: {:?}", v);

                    buf.push(v.as_ref().unwrap().clone());

                    check = buf.as_slice();
                }, 
                None => {
                    #[cfg(debug_assertions)]
                    eprintln!("INEOF");

                    // Does this hunk need to match EOF?
                    if plist.is_empty() && matcheof {
                        break;
                    }

                    if backwarn != 0 && toy.silent {
                        eprintln!("Possibly reversed hunk {} at {}", self.hunknum, self.linenum);
                    }

                    // File ended before we found a place for this hunk.
                    self.fail_hunk(toy)?;
                    // done:
                    for i in buf {
                        do_line(&mut self.outnum, &mut self.state, &mut self.fileout, &i)?;
                    }
                    return Ok(self.state);
                }
            }

            // Compare this line with next expected line of hunk. Match can fail
            // because next line doesn't match, or because we hit end of a hunk that
            // needed EOF and this isn't EOF.
            loop {
                let a = check.first().ok_or_else(|| anyhow!("No line to process!"))?;
                let b = plist.front().ok_or_else(|| anyhow!("No line to process!"))?;
                if plist.is_empty() || lcmp(a, &b[1..]) != Ordering::Equal {
                    // Match failed: can we fuzz it?
                    match plist.front() {
                        Some(d) => {
                            if d.starts_with(|c: char| c.is_ascii_whitespace()) && fuzz < allfuzz {
                                #[cfg(debug_assertions)]
                                eprintln!("FUZZED: {} {}", self.linenum, d);

                                fuzz += 1;

                                // goto: fuzzed
                                // This line matches. Advance plist, detect successful match.
                                plist.pop_front();
                                if plist.is_empty() && !matcheof {
                                    // goto out;
                                    // We have a match.  Emit changed data.
                                    self.state = match reverse {
                                        true => '+' as u32,
                                        false => '-' as u32
                                    };
                                    for line in &self.current_hunk {
                                        if line.starts_with(|c: char| c as u32 == self.state) || line.starts_with(|c: char| c.is_ascii_whitespace()) {
                                            let t: Vec<_> = buf.drain(0..1).collect();
                                            if line.starts_with(|c: char| c.is_ascii_whitespace()) {
                                                let mut f = self.fileout.as_ref().unwrap();
                                                for i in t {
                                                    writeln!(f, "{}", i)?;
                                                }
                                            }
                                        } else {
                                            let mut f = self.fileout.as_ref().unwrap();
                                            writeln!(f, "{}", &line[1..])?;
                                        }
                                    }
                                    self.current_hunk.clear();
                                    self.state = 1;
                                    
                                    for i in buf {
                                        do_line(&mut self.outnum, &mut self.state, &mut self.fileout, &i)?;
                                    }

                                    return Ok(self.state);
                                }
                                check = &check[1..];
                                if check == buf {
                                    break;
                                } 
                            }
                        },
                        _ => {}
                    }

                    #[cfg(debug_assertions)]
                    {
                        let mut bug = 0;

                        if plist.is_empty() {
                            eprintln!("NULL plist");
                        } else {
                            let p = plist.front().ok_or_else(|| anyhow!("[DEBUG] No line to process!"))?;
                            let mut a = check.first().ok_or_else(|| anyhow!("[DEBUG] No line to process!"))?.chars().peekable();
                            let mut b = p.chars().peekable();
                            while a.peek() == b.peek() {
                                bug += 1;
                                a.next();
                                b.next();
                            }
                            eprintln!("NOT({}:{}!={}): {}", bug, &plist.front().unwrap()[bug..],
                            &check.first().unwrap()[bug..], p);
                        }
                    }

                    // If this hunk must match start of file, fail if it didn't.
                    if self.context == 0 || trail > self.context {
                        self.fail_hunk(toy)?;
                        // done:
                        for i in buf {
                            do_line(&mut self.outnum, &mut self.state, &mut self.fileout, &i)?;
                        }
                        return Ok(self.state);
                    }

                    // Write out first line of buffer and recheck rest for new match.
                    self.state = 3;
                    check = &buf[1..];
                    for i in check {
                        do_line(&mut self.outnum, &mut self.state, &mut self.fileout, &i)?;
                    }
                    plist = &mut self.current_hunk;
                    fuzz = 0;

                    // If end of the buffer without finishing a match, read more lines.
                    if buf.is_empty() {
                        break;
                    }

                    check = &buf;
                } else {
                    #[cfg(debug_assertions)]
                    eprintln!("MAYBE: {:?}", plist.front());
                    
                    // fuzzed:
                    // This line matches. Advance plist, detect successful match.
                    plist.pop_front();
                    if plist.is_empty() && !matcheof {
                        // goto out;
                        // We have a match.  Emit changed data.
                        self.state = match reverse {
                            true => '+' as u32,
                            false => '-' as u32
                        };
                        for line in &self.current_hunk {
                            if line.starts_with(|c: char| c as u32 == self.state) || line.starts_with(|c: char| c.is_ascii_whitespace()) {
                                let t: Vec<_> = buf.drain(0..1).collect();
                                if line.starts_with(|c: char| c.is_ascii_whitespace()) {
                                    let mut f = self.fileout.as_ref().unwrap();
                                    for i in t {
                                        writeln!(f, "{}", i)?;
                                    }
                                }
                            } else {
                                let mut f = self.fileout.as_ref().unwrap();
                                writeln!(f, "{}", &line[1..])?;
                            }
                        }
                        self.current_hunk.clear();
                        self.state = 1;
                        
                        for i in buf {
                            do_line(&mut self.outnum, &mut self.state, &mut self.fileout, &i)?;
                        }

                        return Ok(self.state);
                    }
                    check = &check[1..];
                    if check == buf {
                        break;
                    } 
                }
            }
        }
    // out:
        // We have a match.  Emit changed data.
        self.state = match reverse {
            true => '+' as u32,
            false => '-' as u32
        };
        for line in &self.current_hunk {
            if line.starts_with(|c: char| c as u32 == self.state) || line.starts_with(|c: char| c.is_ascii_whitespace()) {
                let t: Vec<_> = buf.drain(0..1).collect();
                if line.starts_with(|c: char| c.is_ascii_whitespace()) {
                    let mut f = self.fileout.as_ref().unwrap();
                    for i in t {
                        writeln!(f, "{}", i)?;
                    }
                }
            } else {
                let mut f = self.fileout.as_ref().unwrap();
                writeln!(f, "{}", &line[1..])?;
            }
        }
        self.current_hunk.clear();
        self.state = 1;
    // done:
        for i in buf {
            do_line(&mut self.outnum, &mut self.state, &mut self.fileout, &i)?;
        }

        return Ok(self.state);
    }
}

fn main() -> Result<()> {
    let mut toy: PatchToy = PatchToy::from_args();

    let mut globals: Globals = Default::default();

    let _reverse = toy.reverse;
    let mut state: u32 = 0;
    let _patchlinenum: isize = 0;
    let _strip: isize = 0;

    let mut oldname: Option<&Path> = None;
    let mut newname: Option<&Path> = None;

    if toy.files.len() == 2 {
        globals.i = Some(&toy.files[1]);
    }

    println!("{:?}", toy);

    match &toy.dir {
        Some(v) => env::set_current_dir(v)?,
        None => {}
    }

    let fp: Option<File> = match globals.i {
        Some(v) => Some(File::open(v)?),
        None => None,
    };

    let filepatch = common::Input::from(fp);

    for p in BufReader::new(filepatch).lines().into_iter() {
        if let Ok(mut patchline) = p {
            // Other versions of patch accept damaged patches, so we need to also.
            // AMY: DOS/Windows '\r' is already handled for us.
            if patchline.starts_with('\0') {
                patchline = String::from(" ");
            }

            // Are we assembling a hunk?
            if state >= 2 {
                if patchline.starts_with(|ch| ch == ' ' || ch == '+' || ch == '-') {
                    globals.current_hunk.push_back(patchline.to_string());

                    if !patchline.starts_with('+') {
                        globals.oldlen -= 1;
                    }

                    if !patchline.starts_with('-') {
                        globals.newlen -= 1;
                    }

                    // Context line?
                    if patchline.starts_with('-') && state == 2 {
                        globals.context += 1;
                    } else {
                        state = 3;
                    }

                    // If we've consumed all expected hunk lines, apply the hunk.
                    if globals.oldlen == 0 && globals.newlen == 0 {
                        state = globals.apply_one_hunk(&toy)?;
                    }
                    continue;
                }
                globals.current_hunk.pop_front();
                globals.fail_hunk(&toy)?;
                state = 0;
                continue;
            }

            // Open a new file?
            if patchline.starts_with("--- ") {
                oldname = None;
                globals.finish_oldfile()?;

                // Trim date from end of filename (if any).  We don't care.
                let s: String = patchline
                    .chars()
                    .skip(4)
                    .skip_while(|c| *c != '\t')
                    .collect();

                match s.parse::<usize>() {
                    Ok(i) => {
                        if i <= 1970 {
                            oldname = Some(DEVNULL());
                        }
                    }
                    Err(_) => {}
                }

                // We defer actually opening the file because svn produces broken
                // patches that don't signal they want to create a new file the
                // way the patch man page says, so you have to read the first hunk
                // and _guess_.

                // Start a new hunk?  Usually @@ -oldline,oldlen +newline,newlen @@
                // but a missing ,value means the value is 1.
            } else if patchline.starts_with("+++ ") {
                newname = None;
                state = 1;

                globals.finish_oldfile()?;

                // Trim date from end of filename (if any).  We don't care.
                let s: String = patchline
                    .chars()
                    .skip(4)
                    .skip_while(|c| *c != '\t')
                    .collect();

                match s.parse::<usize>() {
                    Ok(i) => {
                        if i <= 1970 {
                            newname = Some(DEVNULL());
                        }
                    }
                    Err(_) => {}
                }

                // We defer actually opening the file because svn produces broken
                // patches that don't signal they want to create a new file the
                // way the patch man page says, so you have to read the first hunk
                // and _guess_.

                // Start a new hunk?  Usually @@ -oldline,oldlen +newline,newlen @@
                // but a missing ,value means the value is 1.
            } else if state == 1 && patchline.starts_with("@@ -") {
                let mut i: usize = 0;
                let mut s = patchline.chars().skip(4).peekable();

                // Read oldline[,oldlen] +newline[,newlen]

                globals.oldlen = 1;
                globals.newlen = 1;

                {
                    let x: String = s
                        .by_ref()
                        .skip_while(|c| c.is_ascii_whitespace())
                        .peekable()
                        .peeking_take_while(|c| c.is_ascii_digit())
                        .collect();
                    globals.oldline = x.parse::<usize>()?;
                    if s.by_ref().peek() == Some(&',') {
                        s.by_ref().next();
                        let x: String = s
                            .by_ref()
                            .skip_while(|c| c.is_ascii_whitespace())
                            .peekable()
                            .peeking_take_while(|c| c.is_ascii_digit())
                            .collect();
                        globals.oldlen = x.parse::<usize>()?;
                    }
                }

                s.by_ref().next().ok_or_else(|| anyhow!("Missing data?"))?;
                s.by_ref().next().ok_or_else(|| anyhow!("Missing data?"))?;

                {
                    let x: String = s
                        .by_ref()
                        .skip_while(|c| c.is_ascii_whitespace())
                        .peekable()
                        .peeking_take_while(|c| c.is_ascii_digit())
                        .collect();
                    globals.newline = x.parse::<usize>()?;

                    if s.by_ref().peek() == Some(&',') {
                        s.by_ref().next();
                        let x: String = s
                            .by_ref()
                            .skip_while(|c| c.is_ascii_whitespace())
                            .peekable()
                            .peeking_take_while(|c| c.is_ascii_digit())
                            .collect();
                        globals.newlen = x.parse::<usize>()?;
                    }
                }

                globals.context = 0;
                state = 2;

                // If this is the first hunk, open the file.
                if globals.filein.is_none() {
                    let mut del: usize = 0;
                    let mut name: PathBuf = PathBuf::new();

                    let oldsum = globals.oldline + globals.oldlen;
                    let newsum = globals.newline + globals.newlen;

                    // If an original file was provided on the command line, it overrides
                    // *all* files mentioned in the patch, not just the first.
                    if !toy.files.is_empty() {
                        if _reverse {
                            oldname = Some(toy.files[0].as_path());
                        } else {
                            newname = Some(toy.files[0].as_path());
                        }

                        // The supplied path should be taken literally with or without -p.
                        toy.strip = None;
                    }

                    if toy.reverse {
                        // oldname
                        // We're deleting oldname if new file is /dev/null (before -p)
                        // or if new hunk is empty (zero context) after patching
                        if oldname == Some(DEVNULL()) || oldsum > 0 {
                            name = newname
                                .ok_or_else(|| anyhow!("Undefined old file for removal"))?
                                .to_path_buf();
                            del += 1;
                        }

                        // handle -p path truncation.
                        match toy.strip {
                            Some(v) => {
                                let mut n = name.components();
                                let mut s: Option<&Path> = None;
                                loop {
                                    // XX n.skip(v) moves
                                    match n.next() {
                                        Some(_) => {
                                            if i == v {
                                                break;
                                            }
                                            s = Some(n.as_path());
                                            i += 1;
                                            continue;
                                        }
                                        None => {
                                            break;
                                        }
                                    }
                                }
                                name = s.unwrap().to_path_buf();
                            }
                            None => {}
                        }
                    } else {
                        // newname
                        if newname == Some(DEVNULL()) || newsum > 0 {
                            name = oldname
                                .ok_or_else(|| anyhow!("Undefined new file for removal"))?
                                .to_path_buf();
                            del += 1;
                        }

                        // handle -p path truncation.
                        match toy.strip {
                            Some(v) => {
                                let mut n = name.components();
                                let mut s: Option<&Path> = None;
                                loop {
                                    // XX n.skip(v) moves
                                    match n.next() {
                                        Some(_) => {
                                            if i == v {
                                                break;
                                            }
                                            s = Some(n.as_path());
                                            i += 1;
                                            continue;
                                        }
                                        None => {
                                            break;
                                        }
                                    }
                                }
                                name = s.unwrap().to_path_buf();
                            }
                            None => {}
                        }
                    }

                    if del > 0 {
                        if !toy.silent {
                            println!("removing {}", name.to_string_lossy());
                        }

                        std::fs::remove_file(name)?;

                        state = 0;
                    // If we've got a file to open, do so.
                    } else if toy.strip.is_none() || i <= toy.strip.unwrap_or_default() {
                        // If the old file was null, we're creating a new one.
                        if (oldname == Some(DEVNULL()) || oldsum == 0) && name.exists() {
                            if !toy.silent {
                                println!("creating {}", name.to_string_lossy());
                            }

                            let mkpath = name
                                .parent()
                                .ok_or_else(|| anyhow!("Unknown parent folder for new file"))?;

                            std::fs::create_dir_all(mkpath)?;

                            globals.filein = Some(File::create(&name)?);
                        } else {
                            if !toy.silent {
                                println!("patching {}", name.to_string_lossy());
                            }
                            globals.filein = Some(File::open(&name)?);
                        }
                        if toy.dry_run {
                            globals.fileout =
                                Some(OpenOptions::new().read(true).write(true).open(DEVNULL())?);
                        } else {
                            let x = copy_tempfile(&name)?;
                            globals.tempname = Some(x.0);
                            globals.fileout = Some(x.1);
                        }
                        globals.linenum = 0;
                        globals.outnum = 0;
                        globals.hunknum = 0;
                    }
                }
            }

            globals.hunknum += 1;

            continue;
        }
        // If we didn't continue above, discard this line.
    }

    globals.finish_oldfile()?;

    match globals.exitval {
        Some(v) => Err(anyhow!(v)),
        None => Ok(()),
    }
}
