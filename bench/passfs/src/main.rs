//! Read-only FUSE passthrough of a directory, to measure FUSE's overhead on a hydrated
//! checkout. Attributes and contents come from the backing files; the kernel caches
//! entries, attributes and pages for an hour, as gvc can for its immutable lower layer.
//!
//! Usage: passfs SOURCE MOUNTPOINT [THREADS]
//!
//! PASSFS_NO_OPEN=1 answers open and opendir with ENOSYS, so the kernel stops sending them
//! (zero-message opens) and reads arrive by inode. PASSFS_READDIRPLUS=1 returns attributes
//! with every directory entry, saving `find` a lookup per entry. PASSFS_PASSTHROUGH=1 opens
//! files with FUSE passthrough, so reads bypass the daemon.
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileExt, MetadataExt};
use std::path::PathBuf;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use fuser::{
    Config, Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, INodeNo, InitFlags,
    KernelConfig, LockOwner, MountOption, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory,
    ReplyEmpty, ReplyEntry, ReplyOpen, Request, SessionACL,
};

const TTL: Duration = Duration::from_secs(3600);

struct Inodes {
    paths: Vec<PathBuf>,
    by_path: HashMap<PathBuf, u64>,
}

struct PassFs {
    inodes: RwLock<Inodes>,
    files: RwLock<HashMap<u64, File>>,
    by_ino: RwLock<HashMap<u64, File>>,
    next_fh: AtomicU64,
    no_open: bool,
    plus: bool,
    passthrough: bool,
    backing: RwLock<HashMap<u64, fuser::BackingId>>,
}

fn errno(e: std::io::Error) -> Errno {
    Errno::from_i32(e.raw_os_error().unwrap_or(libc::EIO))
}

fn kind(m: &fs::Metadata) -> FileType {
    let t = m.file_type();
    if t.is_dir() {
        FileType::Directory
    } else if t.is_symlink() {
        FileType::Symlink
    } else {
        FileType::RegularFile
    }
}

fn attr(ino: u64, m: &fs::Metadata) -> FileAttr {
    let t = |s: i64| UNIX_EPOCH + Duration::from_secs(s.max(0) as u64);
    FileAttr {
        ino: INodeNo(ino),
        size: m.len(),
        blocks: m.blocks(),
        atime: t(m.atime()),
        mtime: t(m.mtime()),
        ctime: t(m.ctime()),
        crtime: t(m.mtime()),
        kind: kind(m),
        perm: (m.mode() & 0o7777) as u16,
        nlink: m.nlink() as u32,
        uid: m.uid(),
        gid: m.gid(),
        rdev: 0,
        flags: 0,
        blksize: 4096,
    }
}

impl PassFs {
    fn path(&self, ino: INodeNo) -> Option<PathBuf> {
        self.inodes.read().unwrap().paths.get(ino.0 as usize).cloned()
    }

    fn ino_for(&self, path: PathBuf) -> u64 {
        if let Some(&i) = self.inodes.read().unwrap().by_path.get(&path) {
            return i;
        }
        let mut w = self.inodes.write().unwrap();
        if let Some(&i) = w.by_path.get(&path) {
            return i;
        }
        let i = w.paths.len() as u64;
        w.paths.push(path.clone());
        w.by_path.insert(path, i);
        i
    }
}

impl Filesystem for PassFs {
    fn init(&mut self, _req: &Request, config: &mut KernelConfig) -> std::io::Result<()> {
        let mut flags = InitFlags::FUSE_PARALLEL_DIROPS | InitFlags::FUSE_CACHE_SYMLINKS;
        if self.no_open {
            flags |= InitFlags::FUSE_NO_OPEN_SUPPORT | InitFlags::FUSE_NO_OPENDIR_SUPPORT;
        }
        if self.plus {
            flags |= InitFlags::FUSE_DO_READDIRPLUS;
        }
        if self.passthrough {
            if let Err(e) = config.add_capabilities(InitFlags::FUSE_PASSTHROUGH) {
                eprintln!("passfs: kernel lacks FUSE_PASSTHROUGH: {e:?}");
            }
            let _ = config.set_max_stack_depth(2);
        }
        let _ = config.add_capabilities(flags);
        Ok(())
    }

    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let Some(dir) = self.path(parent) else { return reply.error(Errno::ENOENT) };
        let p = dir.join(name);
        match fs::symlink_metadata(&p) {
            Ok(m) => {
                let ino = self.ino_for(p);
                reply.entry(&TTL, &attr(ino, &m), fuser::Generation(0));
            }
            Err(e) => reply.error(errno(e)),
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        let Some(p) = self.path(ino) else { return reply.error(Errno::ENOENT) };
        match fs::symlink_metadata(&p) {
            Ok(m) => reply.attr(&TTL, &attr(ino.0, &m)),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn readlink(&self, _req: &Request, ino: INodeNo, reply: ReplyData) {
        let Some(p) = self.path(ino) else { return reply.error(Errno::ENOENT) };
        match fs::read_link(&p) {
            Ok(t) => reply.data(t.as_os_str().as_bytes()),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn opendir(&self, _req: &Request, _ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        if self.no_open {
            return reply.error(Errno::ENOSYS);
        }
        reply.opened(FileHandle(0), FopenFlags::empty());
    }

    fn open(&self, _req: &Request, ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        if self.no_open {
            return reply.error(Errno::ENOSYS);
        }
        let Some(p) = self.path(ino) else { return reply.error(Errno::ENOENT) };
        if self.passthrough {
            let f = match File::open(&p) {
                Ok(f) => f,
                Err(e) => return reply.error(errno(e)),
            };
            return match reply.open_backing(f) {
                Ok(id) => {
                    let fh = self.next_fh.fetch_add(1, Ordering::Relaxed);
                    reply.opened_passthrough(FileHandle(fh), FopenFlags::empty(), &id);
                    self.backing.write().unwrap().insert(fh, id);
                }
                Err(e) => {
                    eprintln!("passfs: passthrough open failed: {e}");
                    reply.error(errno(e));
                }
            };
        }
        match File::open(&p) {
            Ok(f) => {
                let fh = self.next_fh.fetch_add(1, Ordering::Relaxed);
                self.files.write().unwrap().insert(fh, f);
                reply.opened(FileHandle(fh), FopenFlags::FOPEN_KEEP_CACHE);
            }
            Err(e) => reply.error(errno(e)),
        }
    }

    fn read(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        if self.no_open {
            if !self.by_ino.read().unwrap().contains_key(&_ino.0) {
                let Some(p) = self.path(_ino) else { return reply.error(Errno::ENOENT) };
                match File::open(&p) {
                    Ok(f) => {
                        self.by_ino.write().unwrap().insert(_ino.0, f);
                    }
                    Err(e) => return reply.error(errno(e)),
                }
            }
            let files = self.by_ino.read().unwrap();
            let f = &files[&_ino.0];
            let mut buf = vec![0u8; size as usize];
            return match f.read_at(&mut buf, offset) {
                Ok(n) => reply.data(&buf[..n]),
                Err(e) => reply.error(errno(e)),
            };
        }
        let files = self.files.read().unwrap();
        let Some(f) = files.get(&fh.0) else { return reply.error(Errno::EBADF) };
        let mut buf = vec![0u8; size as usize];
        match f.read_at(&mut buf, offset) {
            Ok(n) => reply.data(&buf[..n]),
            Err(e) => reply.error(errno(e)),
        }
    }

    fn release(
        &self,
        _req: &Request,
        _ino: INodeNo,
        fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        self.files.write().unwrap().remove(&fh.0);
        reply.ok();
    }

    fn readdirplus(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: fuser::ReplyDirectoryPlus,
    ) {
        let entries = match self.entries(ino) {
            Ok(e) => e,
            Err(e) => return reply.error(e),
        };
        for (i, (child, _k, name, m)) in entries.into_iter().enumerate().skip(offset as usize) {
            let a = match &m {
                Some(m) => attr(child, m),
                None => match fs::symlink_metadata(self.path(INodeNo(child)).unwrap_or_default()) {
                    Ok(m) => attr(child, &m),
                    Err(_) => continue,
                },
            };
            if reply.add(INodeNo(child), (i + 1) as u64, &name, &TTL, &a, fuser::Generation(0)) {
                break;
            }
        }
        reply.ok();
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let Some(dir) = self.path(ino) else { return reply.error(Errno::ENOENT) };
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) => return reply.error(errno(e)),
        };
        let mut entries: Vec<(u64, FileType, std::ffi::OsString)> = vec![
            (ino.0, FileType::Directory, ".".into()),
            (ino.0, FileType::Directory, "..".into()),
        ];
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            entries.push((self.ino_for(e.path()), kind(&m), e.file_name()));
        }
        for (i, (child, k, name)) in entries.into_iter().enumerate().skip(offset as usize) {
            if reply.add(INodeNo(child), (i + 1) as u64, k, &name) {
                break;
            }
        }
        reply.ok();
    }
}

impl PassFs {
    fn entries(&self, ino: INodeNo) -> Result<Vec<(u64, FileType, std::ffi::OsString, Option<fs::Metadata>)>, Errno> {
        let dir = self.path(ino).ok_or(Errno::ENOENT)?;
        let rd = fs::read_dir(&dir).map_err(errno)?;
        let mut entries = vec![
            (ino.0, FileType::Directory, ".".into(), None),
            (ino.0, FileType::Directory, "..".into(), None),
        ];
        for e in rd.flatten() {
            let Ok(m) = fs::symlink_metadata(e.path()) else { continue };
            entries.push((self.ino_for(e.path()), kind(&m), e.file_name(), Some(m)));
        }
        Ok(entries)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = fs::canonicalize(&args[1]).expect("source");
    let threads: usize = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(8);
    // Inode 0 is unused; 1 is the root.
    let inodes = Inodes { paths: vec![PathBuf::new(), src.clone()], by_path: HashMap::from([(src, 1)]) };
    let on = |k: &str| std::env::var(k).is_ok_and(|v| v == "1");
    let fs = PassFs {
        inodes: RwLock::new(inodes),
        files: RwLock::new(HashMap::new()),
        by_ino: RwLock::new(HashMap::new()),
        next_fh: AtomicU64::new(1),
        no_open: on("PASSFS_NO_OPEN"),
        plus: on("PASSFS_READDIRPLUS"),
        passthrough: on("PASSFS_PASSTHROUGH"),
        backing: RwLock::new(HashMap::new()),
    };
    let mut cfg = Config::default();
    cfg.mount_options = vec![MountOption::RO, MountOption::FSName("passfs".into())];
    cfg.acl = SessionACL::Owner;
    cfg.n_threads = Some(threads);
    cfg.clone_fd = true;
    fuser::mount(fs, &args[2], &cfg).expect("mount");
}
