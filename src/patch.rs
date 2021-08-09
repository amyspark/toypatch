mod common;

use crate::common::*;
use anyhow::{anyhow, Result};
use log::debug;
use peeking_take_while::PeekableExt;
use std::env;
use std::ffi::{OsStr};
use std::fs;
use std::fs::{File, OpenOptions};
use std::io;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use structopt::StructOpt;

/// Apply a unified diff to one or more files.
///
/// This version of patch only handles unified diffs, and only modifies
/// a file when all hunks to that file apply. Patch prints failed hunks
/// to stderr, and exits with nonzero status if any hunks fail.
///
/// A file compared against `/dev/null` (or with a date <= the epoch) is
/// created/deleted as appropriate.
#[derive(Debug, StructOpt)]
struct PatchToy {
    /// Modify files in `dir`
    #[structopt(short)]
    dir: Option<PathBuf>,

    /// Input patch file (default = stdin)
    #[structopt(short)]
    input: Option<PathBuf>,

    /// Number of '/' to strip from start of file paths (default = all)
    #[structopt(short = "p")]
    strip: Option<usize>,

    /// Reverse patch
    #[structopt(short = "R")]
    reverse: bool,

    /// Silent except for errors
    #[structopt(short)]
    silent: bool,

    /// Ignored (only handles "unified" diffs)
    #[structopt(short)]
    _unified: bool,

    /// Don't change files, just confirm patch applies
    #[structopt(long)]
    dry_run: bool,

    /// Pairs of file and patch to apply.
    #[structopt(parse(from_os_str))]
    files: Vec<PathBuf>,
}

#[derive(Default, Debug)]
struct Globals<'a> {
    i: Option<&'a PathBuf>,
    d: Option<&'a str>,

    p: usize,
    g: usize,
    f: usize,

    current_hunk: Vec<String>,
    oldline: usize,
    oldlen: usize,
    newline: usize,
    newlen: usize,
    linenum: isize,
    outnum: isize,

    context: isize,
    state: isize,
    filein: Option<File>,
    fileout: Option<File>,
    hunknum: isize,
    tempname: Option<PathBuf>,
    destname: Option<PathBuf>,
}

impl Globals<'_> {
    /// Dispose of a line of input, either by writing it out or discarding it.
    ///
    /// state < 2: just free
    ///
    /// state = 2: write whole line to stderr
    ///
    /// state = 3: write whole line to fileout
    ///
    /// state > 3: write line+1 to fileout when *line != state
    pub fn do_line(&mut self, data: &str) -> Result<()> {
        self.outnum += 1;
        if self.state > 1 {
            if self.state == 2 {
                if self.state > 3 {
                    eprintln!("{}", &data[1..]);
                } else {
                    eprintln!("{}", &data[0..]);
                }
            } else {
                let mut f = self.fileout.as_ref().unwrap();
                if self.state > 3 {
                    writeln!(f, "{}", &data[1..])?;
                } else {
                    writeln!(f, "{}", &data[0..])?;
                }
            }
        }

        debug!("DO {}: {}", self.state, data);

        Ok(())
    }

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

    pub fn fail_hunk(&mut self) {
        // if (!TT.current_hunk) return;

        // fprintf(stderr, "Hunk %d FAILED %ld/%ld.\n",
        //     TT.hunknum, TT.oldline, TT.newline);
        // toys.exitval = 1;

        // // If we got to this point, we've seeked to the end.  Discard changes to
        // // this file and advance to next file.

        // TT.state = 2;
        // llist_traverse(TT.current_hunk, do_line);
        // TT.current_hunk = NULL;
        // if (!FLAG(dry_run)) delete_tempfile(TT.filein, TT.fileout, &TT.tempname);
        // TT.state = 0;
    }

    /// Given a hunk of a unified diff, make the appropriate change to the file.
    /// This does not use the location information, but instead treats a hunk
    /// as a sort of regex. Copies data from input to output until it finds
    /// the change to be made, then outputs the changed data and returns.
    /// (Finding EOF first is an error.) This is a single pass operation, so
    /// multiple hunks must occur in order in the file.
    pub fn apply_one_hunk(&mut self) -> isize {
        // struct double_list *plist, *buf = 0, *check;
        // int matcheof, trail = 0, reverse = FLAG(R), backwarn = 0, allfuzz, fuzz, i;
        // int (*lcmp)(char *aa, char *bb) = FLAG(l) ? (void *)loosecmp : (void *)strcmp;

        // // Match EOF if there aren't as many ending context lines as beginning
        // dlist_terminate(TT.current_hunk);
        // for (fuzz = 0, plist = TT.current_hunk; plist; plist = plist->next) {
        //     char c = *plist->data, *s;

        //     if (c==' ') trail++;
        //     else trail = 0;

        //     // Only allow fuzz if 2 context lines have multiple nonwhitespace chars.
        //     // avoids the "all context was blank or } lines" issue. Removed lines
        //     // count as context since they're matched.
        //     if (c==' ' || c=="-+"[reverse]) {
        //     s = plist->data+1;
        //     while (isspace(*s)) s++;
        //     if (*s && s[1] && !isspace(s[1])) fuzz++;
        //     }

        //     if (FLAG(x)) fprintf(stderr, "HUNK:%s\n", plist->data);
        // }
        // matcheof = !trail || trail < TT.context;
        // if (fuzz<2) allfuzz = 0;
        // else allfuzz = FLAG(F) ? TT.F : (TT.context ? TT.context-1 : 0);

        // if (FLAG(x)) fprintf(stderr,"MATCHEOF=%c\n", matcheof ? 'Y' : 'N');

        // // Loop through input data searching for this hunk. Match all context
        // // lines and lines to be removed until we've found end of complete hunk.
        // plist = TT.current_hunk;
        // fuzz = 0;
        // for (;;) {
        //     char *data = get_line(TT.filein);

        //     // Figure out which line of hunk to compare with next. (Skip lines
        //     // of the hunk we'd be adding.)
        //     while (plist && *plist->data == "+-"[reverse]) {
        //     if (data && !lcmp(data, plist->data+1))
        //         if (!backwarn) backwarn = TT.linenum;
        //     plist = plist->next;
        //     }

        //     // Is this EOF?
        //     if (!data) {
        //     if (FLAG(x)) fprintf(stderr, "INEOF\n");

        //     // Does this hunk need to match EOF?
        //     if (!plist && matcheof) break;

        //     if (backwarn && !FLAG(s))
        //         fprintf(stderr, "Possibly reversed hunk %d at %ld\n",
        //             TT.hunknum, TT.linenum);

        //     // File ended before we found a place for this hunk.
        //     fail_hunk();
        //     goto done;
        //     } else {
        //     TT.linenum++;
        //     if (FLAG(x)) fprintf(stderr, "IN: %s\n", data);
        //     }
        //     check = dlist_add(&buf, data);

        //     // Compare this line with next expected line of hunk. Match can fail
        //     // because next line doesn't match, or because we hit end of a hunk that
        //     // needed EOF and this isn't EOF.
        //     for (i = 0;; i++) {
        //     if (!plist || lcmp(check->data, plist->data+1)) {

        //         // Match failed: can we fuzz it?
        //         if (plist && *plist->data == ' ' && fuzz<allfuzz) {
        //         if (FLAG(x))
        //             fprintf(stderr, "FUZZED: %ld %s\n", TT.linenum, plist->data);
        //         fuzz++;

        //         goto fuzzed;
        //         }

        //         if (FLAG(x)) {
        //         int bug = 0;

        //         if (!plist) fprintf(stderr, "NULL plist\n");
        //         else {
        //             while (plist->data[bug] == check->data[bug]) bug++;
        //             fprintf(stderr, "NOT(%d:%d!=%d): %s\n", bug, plist->data[bug],
        //             check->data[bug], plist->data);
        //         }
        //         }

        //         // If this hunk must match start of file, fail if it didn't.
        //         if (!TT.context || trail>TT.context) {
        //         fail_hunk();
        //         goto done;
        //         }

        //         // Write out first line of buffer and recheck rest for new match.
        //         TT.state = 3;
        //         do_line(check = dlist_pop(&buf));
        //         plist = TT.current_hunk;
        //         fuzz = 0;

        //         // If end of the buffer without finishing a match, read more lines.
        //         if (!buf) break;
        //         check = buf;
        //     } else {
        //         if (FLAG(x)) fprintf(stderr, "MAYBE: %s\n", plist->data);
        // fuzzed:
        //         // This line matches. Advance plist, detect successful match.
        //         plist = plist->next;
        //         if (!plist && !matcheof) goto out;
        //         check = check->next;
        //         if (check == buf) break;
        //     }
        //     }
        // }
        // out:
        // // We have a match.  Emit changed data.
        // TT.state = "-+"[reverse];
        // while ((plist = dlist_pop(&TT.current_hunk))) {
        //     if (TT.state == *plist->data || *plist->data == ' ') {
        //     if (*plist->data == ' ') dprintf(TT.fileout, "%s\n", buf->data);
        //     llist_free_double(dlist_pop(&buf));
        //     } else dprintf(TT.fileout, "%s\n", plist->data+1);
        //     llist_free_double(plist);
        // }
        // TT.current_hunk = 0;
        // TT.state = 1;
        // done:
        // llist_traverse(buf, do_line);

        return self.state;
    }
}

fn main() -> Result<()> {
    let mut toy: PatchToy = PatchToy::from_args();

    let mut globals: Globals = Default::default();

    let _reverse = toy.reverse;
    let mut state: isize = 0;
    let _patchlinenum: isize = 0;
    let _strip: isize = 0;

    let mut oldname: Option<&Path> = None;
    let mut newname: Option<&Path> = None;

    if toy.files.len() == 2 {
        globals.i = Some(&toy.files[1]);
    }

    globals.filein = None;
    globals.fileout = None;

    println!("{:?}", toy);

    match toy.dir {
        Some(v) => env::set_current_dir(v)?,
        None => {}
    }

    let fp: Option<&Path> = match globals.i {
        Some(v) => Some(Path::new(v)),
        None => None,
    };

    let filepatch = common::Input::new(fp)?;

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
                    globals.current_hunk.push(patchline.to_string());

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
                        state = globals.apply_one_hunk();
                    }
                    continue;
                }
                globals.current_hunk.pop();
                globals.fail_hunk();
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
                    let mut name: &Path = Path::new("");

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
                        toy.strip = Some(0);
                    }

                    if toy.reverse { // oldname
                        // We're deleting oldname if new file is /dev/null (before -p)
                        // or if new hunk is empty (zero context) after patching
                        if oldname == Some(DEVNULL()) || oldsum > 0 {
                            name = newname.ok_or_else(|| anyhow!("Undefined old file for removal"))?;
                            del += 1;
                        }

                        // handle -p path truncation.
                        match toy.strip {
                            Some(v) => {
                                let mut n = name.components();
                                n.skip(v);
                                name = n.as_path();
                            },
                            None => {},
                        }
                    } else { // newname
                        if newname == Some(DEVNULL()) || newsum > 0 {
                            name = oldname.ok_or_else(|| anyhow!("Undefined new file for removal"))?;
                            del += 1;
                        }

                        // handle -p path truncation.
                        match toy.strip {
                            Some(v) => {
                                let mut n = name.components();
                                n.skip(v);
                                name = n.as_path();
                            },
                            None => {},
                        }
                    }

                    if del > 0 {
                        if !toy.silent {
                            println!("removing {:?}", name);
                        }

                        std::fs::remove_file(name)?;

                        state = 0;
                    // If we've got a file to open, do so.
                    } else if toy.strip.is_none() || i <= toy.strip.unwrap_or_default() {
                        // If the old file was null, we're creating a new one.
                        if (oldname == Some(DEVNULL()) || oldsum == 0) && name.exists() {
                            if !toy.silent {
                                println!("creating {:?}", name);
                            }

                            let mkpath = name
                                .parent()
                                .ok_or_else(|| anyhow!("Unknown parent folder for new file"))?;

                            std::fs::create_dir_all(mkpath)?;

                            globals.filein = Some(File::create(name)?);
                        } else {
                            if !toy.silent {
                                println!("patching {:?}", name);
                            }
                            globals.filein = Some(File::open(name)?);
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

    Ok(())
}
