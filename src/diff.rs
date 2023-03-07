use anyhow::{anyhow, Context, Result, bail};
use clap::{Parser};
use std::cmp::{Ordering};
use std::convert::{TryFrom};
use std::path::{Path, PathBuf};
use std::fs;
use std::process;

/// diff - compare files line by line
#[derive(Default, Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Treat all files as text
    #[clap(short = 'a')]
    text: bool,

    /// Ignore changes in the amount of whitespace
    #[clap(short = 'b')]
    ignore_space_change: bool,

    /// Ignore changes whose lines are all blank
    #[clap(short = 'B')]
    ignore_blank_lines: bool,

    // Try hard to find a smaller set of changes
    #[clap(short = 'd')]
    minimal: bool,

    /// Ignore case differences
    #[clap(short = 'i')]
    ignore_case: bool,

    /// Use LABEL instead of the filename in the unified header
    #[clap(short = 'L')]
    label: Option<String>,

    /// Treat absent files as empty
    #[clap(short = 'N')]
    new_file: bool,

    /// Output only whether files differ
    #[clap(short = 'q')]
    brief: bool,

    /// Recurse
    #[clap(short = 'r')]
    recurse: bool,

    /// Start with FILE when comparing directories
    #[clap(short = 'S')]
    starting_file: Option<PathBuf>,

    /// Make tabs line up by prefixing a tab when necessary
    #[clap(short = 'T')]
    initial_tab: bool,

    /// Report when two files are the same
    #[clap(short = 's')]
    report_identical_files: bool,

    /// Expand tabs to spaces in output
    #[clap(short = 't')]
    expand_tabs: bool,

    /// Output LINES lines of context
    #[clap(short = 'U')]
    unified: Option<i32>,

    /// Ignore all whitespace
    #[clap(short = 'w')]
    ignore_all_space: bool,

    /// Colored output
    #[clap(long)]
    color: bool,

    /// Strip trailing '\r's from input lines
    #[clap(long)]
    strip_trailing_cr: bool,

    /// File to be compared against
    #[clap()]
    file1: PathBuf,

    /// File to be compared
    #[clap()]
    file2: PathBuf
}

enum Status {
    SAME,
    DIFFER
}

impl Default for Status {
    fn default() -> Status{
        return Status::SAME;
    }
}

///
#[derive(Default)]
struct Globals {
    ///
    exitval: i32,

    ///
    ct: i64,

    ///
    start: String,

    ///
    optflags: Args,

    ///
    dir_num: i64,

    ///
    size: i64,

    ///
    is_binary: bool,

    ///
    status: Status,

    ///
    change: i64,

    /// Length of the root paths for each dir entry.
    len: [PathBuf; 2],

    ///
    offset: [i64; 2],

    ///
    st: [Metadata; 2],

    /// List of directories and files under the specified paths.
    dir: [Vec<walkdir::DirEntry>; 2]
}

#[derive(Default)]
struct Metadata {
    metadata: Option<fs::Metadata>
}

#[derive(Default)]
struct Diff {
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    prev: i64, 
    suff: i64
}

impl Metadata {
    fn is_dir(&self) -> bool {
        match &self.metadata {
            Some(v) => return v.is_dir(),
            None => return false
        }
    }
}

impl TryFrom<&PathBuf> for Metadata {
    type Error = anyhow::Error;
    fn try_from(p: &PathBuf) -> Result<Metadata, Self::Error> {
        return Ok(Metadata{metadata: Some(fs::metadata(p)?)});
    }
}



fn is_a_tty(stderr: bool) -> bool {
    let stream = if stderr {
        atty::Stream::Stderr
    } else {
        atty::Stream::Stdout
    };

    atty::is(stream)
}

fn is_stdin(p: &PathBuf) -> bool {
    return p.to_string_lossy() == "-";
}

#[allow(non_snake_case)]
fn do_diff(files: &Vec<PathBuf>, TT: &Globals) {
    let mut i: i64 = 1;
    let mut size: i64 = 1;
    let mut x: i64 = 0;
    let mut change: i64 = 0;
    let mut ignore_white: i64 = 0;
    let mut start1: i64 = 0;
    let mut end1: i64 = 0;
    let mut start2: i64 = 0;
    let mut end2: i64 = 0;

    let mut d: Diff = Default::default();

    let llist: &Args = &TT.optflags;

    TT.offset[0] = 0;
    TT.offset[1] = 0;

    let mut J = diff(files);
 
    if J != 0 {
        return //No need to compare, have to status only
    }
 
//    d = xzalloc(size *sizeof(struct diff));
//    do {
//      ignore_white = 0;
//      for (d[x].a = i; d[x].a <= file[0].len; d[x].a++) {
//        if (J[d[x].a] != (J[d[x].a - 1] + 1)) break;
//        else continue;
//      }
//      d[x].c = (J[d[x].a - 1] + 1);
 
//      for (d[x].b = (d[x].a - 1); d[x].b <= file[0].len; d[x].b++) {
//        if (J[d[x].b + 1]) break;
//        else continue;
//      }
//      d[x].d = (J[d[x].b + 1] - 1);
 
//      if ((toys.optflags & FLAG_B)) {
//        if (d[x].a <= d[x].b) {
//          if ((TT.offset[0][d[x].b] - TT.offset[0][d[x].a - 1])
//              == (d[x].b - d[x].a + 1))
//            ignore_white = 1;
//        } else if (d[x].c <= d[x].d){
//          if ((TT.offset[1][d[x].d] - TT.offset[1][d[x].c - 1])
//              == (d[x].d - d[x].c + 1))
//            ignore_white = 1;
//        }
//      }
 
//      if ((d[x].a <= d[x].b || d[x].c <= d[x].d) && !ignore_white)
//        change = 1; //is we have diff ?
 
//      if (!ignore_white) d = xrealloc(d, (x + 2) *sizeof(struct diff));
//      i = d[x].b + 1;
//      if (i > file[0].len) break;
//      J[d[x].b] = d[x].d;
//      if (!ignore_white) x++;
//    } while (i <= file[0].len);
 
//    i = x+1;
//    TT.status = change; //update status, may change bcoz of -w etc.
 
//    if (!(toys.optflags & FLAG_q) && change) {  //start of !FLAG_q
//      if (toys.optflags & FLAG_color) printf("\e[1m");
//      if (toys.optflags & FLAG_L) printf("--- %s\n", llist->arg);
//      else show_label("---", files[0], &(TT).st[0]);
//      if (((toys.optflags & FLAG_L) && !llist->next) || !(toys.optflags & FLAG_L))
//        show_label("+++", files[1], &(TT).st[1]);
//      else {
//        while (llist->next) llist = llist->next;
//        printf("+++ %s\n", llist->arg);
//      }
//      if (toys.optflags & FLAG_color) printf("\e[0m");
 
//      struct diff *t, *ptr1 = d, *ptr2 = d;
//      while (i) {
//        long a,b;
 
//        if (TT.ct > file[0].len) TT.ct = file[0].len; //trim context to file len.
//        if (ptr1->b < ptr1->a && ptr1->d < ptr1->c) {
//          i--;
//          continue;
//        }
//        //Handle the context stuff
//        a =  ptr1->a;
//        b =  ptr1->b;
 
//        b  = MIN(file[0].len, b);
//        if (i == x + 1) ptr1->suff = MAX(1,a - TT.ct);
//        else {
//          if ((ptr1 - 1)->prev >= (ptr1->a - TT.ct))
//            ptr1->suff = (ptr1 - 1)->prev + 1;
//          else ptr1->suff =  ptr1->a - TT.ct;
//        }
//  calc_ct:
//        if (i > 1) {
//          if ((ptr2->b + TT.ct) >= (ptr2  + 1)->a) {
//            ptr2++;
//            i--;
//            goto calc_ct;
//          } else ptr2->prev = ptr2->b + TT.ct;
//        } else ptr2->prev = ptr2->b;
//        start1 = (ptr2->prev - ptr1->suff + 1);
//        end1 = (start1 == 1) ? -1 : start1;
//        start2 = MAX(1, ptr1->c - (ptr1->a - ptr1->suff));
//        end2 = ptr2->prev - ptr2->b + ptr2->d;
 
//        if (toys.optflags & FLAG_color) printf("\e[36m");
//        printf("@@ -%ld", start1 ? ptr1->suff: (ptr1->suff -1));
//        if (end1 != -1) printf(",%ld ", ptr2->prev-ptr1->suff + 1);
//        else putchar(' ');
 
//        printf("+%ld", (end2 - start2 + 1) ? start2: (start2 -1));
//        if ((end2 - start2 +1) != 1) printf(",%ld ", (end2 - start2 +1));
//        else putchar(' ');
//        printf("@@");
//        if (toys.optflags & FLAG_color) printf("\e[0m");
//        putchar('\n');
 
//        for (t = ptr1; t <= ptr2; t++) {
//          if (t== ptr1) print_diff(t->suff, t->a-1, ' ', TT.offset[0], file[0].fp);
//          print_diff(t->a, t->b, '-', TT.offset[0], file[0].fp);
//          print_diff(t->c, t->d, '+', TT.offset[1], file[1].fp);
//          if (t == ptr2)
//            print_diff(t->b+1, (t)->prev, ' ', TT.offset[0], file[0].fp);
//          else print_diff(t->b+1, (t+1)->a-1, ' ', TT.offset[0], file[0].fp);
//        }
//        ptr2++;
//        ptr1 = ptr2;
//        i--;
//      } //end of while
//    } //End of !FLAG_q
//    free(d);
//    free(J);
//    free(TT.offset[0]);
//    free(TT.offset[1]);
}

#[allow(non_snake_case)]
fn show_status(files: &Vec<PathBuf>, TT: &Globals) {
  match TT.status {
    Status::SAME => {
      if TT.optflags.report_identical_files {
        println!("Files {} and {} are identical", files[0].to_string_lossy(), files[1].to_string_lossy());
      }
    }
    Status::DIFFER => {
        if TT.optflags.brief || TT.is_binary {
            println!("Files {} and {} differ", files[0].to_string_lossy(), files[1].to_string_lossy())
        }
    }
  }
}

fn concat_file_path(path: &Path, default_path: &Path) -> PathBuf {
    let mut final_path = path.to_path_buf();
    if path.ends_with(std::path::MAIN_SEPARATOR.to_string()) {
        if default_path.is_relative() {
            final_path.push(default_path);
        }
        else {
            let mut t = default_path.components();
            t.next();
            final_path.push(t.as_path());
        }
    }
    else if default_path.is_relative() {
        final_path.push(default_path);
    }
    else {
        let mut t = default_path.components();
        t.next();
        final_path.push(t.as_path());
    }

    final_path
}

fn create_empty_entry(l: usize, r: usize, j: Ordering, TT: &Globals) -> Result<()> {
    let mut st: Vec<fs::Metadata> = Default::default();
    let mut f: Vec<PathBuf> = Default::default();
    let mut path: Vec<PathBuf> = Default::default();

    if j == Ordering::Greater && TT.optflags.new_file {
        path[0] = concat_file_path(&TT.len[0],
            TT.dir[1][r]
            .path()
            .strip_prefix(&TT.len[1])?);
        f[0] = Path::new("/dev/null").to_path_buf();
        f[1] = TT.dir[1][r].path().to_path_buf();
        path[1] = f[1].to_path_buf();
        st.insert(0, fs::metadata(&f[1])?);
        st.insert(1, st[0].clone());
    }
    else if j == Ordering::Less && TT.optflags.new_file {
        path[1] = concat_file_path(&TT.len[0], TT.dir[0][l].path().strip_prefix(&TT.len[0])?);
        f[1] = Path::new("/dev/null").to_path_buf();
        f[0] = TT.dir[0][l].path().to_path_buf();
        path[0] = f[0].to_path_buf();
        st.insert(0, fs::metadata(&f[0])?);
        st.insert(1, st[0].clone());
    }

    if j == Ordering::Equal {
        for i in 0..2 {
            f[i] = match i == 0 {
                true => TT.dir[i][l].path().to_path_buf(),
                false => TT.dir[i][r].path().to_path_buf()
            };
            path[i] = f[i].to_path_buf();
            st[i] = fs::metadata(&f[i])?;
        }
    }

    if st[0].is_dir() && st[1].is_dir() {
        println!("Common subdirectories: {:?} and {:?}", path[0], path[1]);
    } else if !st[0].is_file() && !st[0].is_dir() {
        println!("File {:?} is not a regular file or directory and was skipped", path[0]);
    } else if !st[1].is_file() && !st[1].is_dir() {
        println!("File {:?} is not a regular file or directory and was skipped", path[1]);
    } else if st[0].is_dir() != st[1].is_dir() {
        if st[0].is_dir() {
            println!("File {:?} is a {} while file {:?} is a {}", path[0], "directory", path[1], "regular file");
        } else {
            println!("File {:?} is a {} while file {:?} is a {}", path[0], "regular file", path[1], "directory");
        }
    } else {
        do_diff(&f, &TT);
        show_status(&path, &TT);
    }

    Ok(())
}

#[allow(non_snake_case)]
fn diff_dir(start: &[usize; 2], TT: &mut Globals) -> Result<()> {

    // left side file start
    let mut l: usize = start[0];
    // right side file start
    let mut r: usize = start[1];

    
    while l < TT.dir[0].len() && r < TT.dir[1].len() {
        let f0 = TT.dir[0][l].path().strip_prefix(&TT.len[0])?;
        let f1 = TT.dir[1][l].path().strip_prefix(&TT.len[1])?;

        let j = f0.partial_cmp(f1).context("Unable to order files")?;

        if !TT.optflags.new_file {
            match j {
                Ordering::Greater => {
                    println!("Only in {:?}: {:?}", TT.len[0], f0);
                    r += 1;
                },
                _ => {
                    println!("Only in {:?}: {:?}", TT.len[1], f1);
                    l += 1;
                }
            }

            TT.status = Status::DIFFER;
        }
        else {
            create_empty_entry(l, r, j, &TT)?; //create non empty dirs/files if -N.

            match j {
                Ordering::Greater => {
                    r += 1;
                },
                Ordering::Less => {
                    l += 1;
                },
                Ordering::Equal => {
                    l += 1;
                    r += 1;
                }
            }
        }
    }

  if l == TT.dir[0].len() {
    while r < TT.dir[1].len() {
        if TT.optflags.new_file {
            println!("Only in {}: {}", TT.dir[1][0].path().to_string_lossy(), TT.dir[1][r].path().strip_prefix(&TT.len[1])?.to_string_lossy());
            TT.status = Status::DIFFER;
        } else {
            create_empty_entry(l, r, Ordering::Greater, TT)?;
        }
        TT.dir[1].remove(r);
        r += 1;
    }
  } else if r == TT.dir[1].len() {
    while l < TT.dir[0].len() {
        if TT.optflags.new_file {
        println!("Only in {}: {}", TT.dir[1][0].path().to_string_lossy(), TT.dir[0][l].path().strip_prefix(&TT.len[0])?.to_string_lossy());
        TT.status = Status::DIFFER;
      } else {
        create_empty_entry(l, r, Ordering::Less, TT)?;
      }
      TT.dir[0].remove(l);
      l += 1;
    }
  }

    Ok(())
}

fn diff_main(flags: Args) -> Result<Status>{
    #[allow(non_snake_case)]
    let mut TT: Globals = Globals{ optflags: flags, ..Default::default()};

    let mut start: [usize; 2] = [1, 1];

    let mut files: Vec<PathBuf> = Default::default();

    if TT.optflags.color && !is_a_tty(true) {
        TT.optflags.color = false;
    }

    {
        files.insert(0, TT.optflags.file1.clone());

        TT.st[0] = match is_stdin(&files[0]) {
            // XXX: How do I fstat stdin in Rust?
            true => Default::default(),
            false => Metadata::try_from(&files[0])?
        }
    }

    {
        files.insert(1, TT.optflags.file2.clone());

        TT.st[1] = match is_stdin(&files[1]) {
            // XXX: How do I fstat stdin in Rust?
            true => Default::default(),
            false => Metadata::try_from(&files[1])?
        }
    }

    if is_stdin(&files[0]) || is_stdin(&files[1]) {
        if TT.st[0].is_dir() {
            bail!("can't compare stdin to directory")
        }
        if TT.st[1].is_dir() {
            bail!("can't compare stdin to directory")
        }
    }

    /// physically same device
    #[cfg(unix)]
    {
        if TT.st[0].ino() == TT.st[1].ino() {
            TT.status = Status::SAME;
            show_status(files, TT);
            return Ok(TT.status);
        }
    }

    #[cfg(windows)]
    {
        if fs::canonicalize(&files[0])? == fs::canonicalize(&files[1])? {
            TT.status = Status::SAME;
            show_status(&files, &TT);
            return Ok(TT.status);
        }
    }

    if TT.st[0].is_dir() && TT.st[1].is_dir() {
        // Here it attempts to list both directories recursively,
        // following symlinks and sorting by name...?

        TT.dir[0] = walkdir::WalkDir::new(&files[0])
            .follow_links(true)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|e| e.ok())
            .collect::<Vec<_>>();
        
        TT.len[0] = TT.dir[0].first().context("no first directory path")?.path().to_path_buf();

        TT.dir[1] = walkdir::WalkDir::new(&files[1])
            .follow_links(true)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|e| e.ok())
            .collect::<Vec<_>>();

        TT.len[1] = TT.dir[0].first().context("no first directory path")?.path().to_path_buf();

        // need to check every pathname whose last bit matches v
        match &TT.optflags.starting_file {
            Some(v) => {
                start[0] = TT.dir[0]
                    .iter()
                    .position(|i| i.file_name() >= v)
                    .unwrap_or(0);
                start[1] = TT.dir[1]
                    .iter()
                    .position(|i| i.file_name() >= v)
                    .unwrap_or(0);
            },
            None => {}
        }

        TT.dir_num = 2;
        TT.size = 0;

        diff_dir(&start, &mut TT)?;
    }
    else {
        if TT.st[0].is_dir() || TT.st[1].is_dir() {
            let d = TT.st[0].is_dir() as usize;

            files[1 - d] = files[1 - d].with_file_name(&files[d]);

            TT.st[1 - d] = match fs::metadata(&files[1-d]) {
                Ok(v) => Metadata {metadata: Some(v)},
                Err(e) => {
                    bail!(e)
                }
            }
        }

        do_diff(&files, &TT);
        show_status(&files, &TT);
    }

    return Ok(TT.status);
}

fn main() -> Result<()> {
    let optflags = Args::parse();

    match diff_main(optflags) {
        Ok(v) => process::exit(v as i32),
        Err(v) => {
            anyhow!("diff: {}", v);
            process::exit(2);
        }
    }
}
