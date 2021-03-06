// This is a part of Sonorous.
// Copyright (c) 2005, 2007, 2009, 2012, 2013, 2014, Kang Seonghoon.
// See README.md and LICENSE.txt for details.

//! Caches backed by the external database.

use std::path;
use std::io::{IoError, OtherIoError, IoResult, FileType, FileStat, SeekSet};
use std::io::fs::{PathExtensions, File, readdir};
use util::md5::{MD5, MD5Hash};
use format::metadata::{Level, Difficulty, Meta};

use sqlite3;
use sqlite3::{ResultCode, ColumnType, BindArg};

/// Encodes a path (possibly) relative to the root path as a byte vector.
fn encode_path(root: &Path, path: &Path) -> Vec<u8> {
    let normalized0 = path.path_relative_from(root);
    let normalized = normalized0.as_ref().unwrap_or(path);
    let mut ret = normalized.root_path().unwrap_or(Path::new(".")).into_vec();
    for component in normalized.components() {
        ret.push(0);
        ret.extend(component.iter().map(|&s| s));
    }
    ret
}

#[test]
fn test_encode_path() {
    fn path(cs: &[&str]) -> Path {
        if cs.len() == 1 {
            Path::new(cs[0])
        } else {
            Path::new(cs[0]).join_many(cs[1..])
        }
    }

    let root = path([".", "a", "b"]);
    assert_eq!(encode_path(&root, &path([".", "c", "d"]))[], b".\0..\0..\0c\0d");
    assert_eq!(encode_path(&root, &path([".", "c", "d", ""]))[], b".\0..\0..\0c\0d");
    assert_eq!(encode_path(&root, &path([".", "a", "e"]))[], b".\0..\0e");
    assert_eq!(encode_path(&root, &path([".", "a", "b", "f"]))[], b".\0f");

    if cfg!(target_os = "windows") {
        assert_eq!(encode_path(&root, &path(["\\", "x", "y", "z"]))[], b"\\\0x\0y\0z");
        assert_eq!(encode_path(&root, &path(["C:\\", "x", "y", "z"]))[], b"C:\\\0x\0y\0z");
        assert_eq!(encode_path(&root, &path(["c:\\", "x", "y", "z"]))[], b"C:\\\0x\0y\0z");
        assert_eq!(encode_path(&root, &path(["C:.", "x", "y", "z"]))[], b"C:\0x\0y\0z");
        assert_eq!(encode_path(&root, &path(["c:.", "x", "y", "z"]))[], b"C:\0x\0y\0z");

        let absroot = path(["C:\\", "a", "b"]);
        assert_eq!(encode_path(&absroot, &path(["C:\\", "c", "d"]))[], b".\0..\0..\0c\0d");
        assert_eq!(encode_path(&absroot, &path(["D:\\", "c", "d"]))[], b"D:\\\0c\0d");
    } else {
        assert_eq!(encode_path(&root, &path(["/", "x", "y", "z"]))[], b"/\0x\0y\0z");

        let absroot = path(["/", "a", "b"]);
        assert_eq!(encode_path(&absroot, &path(["/", "c", "d"]))[], b".\0..\0..\0c\0d");
        // this is why the caller should use the absolute path if possible
        assert_eq!(encode_path(&absroot, &path([".", "c", "d"]))[], b".\0c\0d");
    }
}

/// Converts an SQLite `ResultCode` into an `IoError`.
fn io_error_from_sqlite(db: Option<&sqlite3::Database>, code: sqlite3::ResultCode) -> IoError {
    let detail = match db {
        Some(db) => format!("{} - {}", code, db.get_errmsg()),
        None => format!("{}", code),
    };
    IoError { kind: OtherIoError, desc: "SQLite error", detail: Some(detail) }
}

/// Calls `panic!` with an SQLite `ResultCode`.
fn fail_from_sqlite(db: &sqlite3::Database, code: sqlite3::ResultCode) -> ! {
    panic!("SQLite error: {}", io_error_from_sqlite(Some(db), code));
}

/// `try!`-friendly version of `Cursor::step`.
fn step_cursor(db: &sqlite3::Database, c: &mut sqlite3::Cursor) -> IoResult<bool> {
    match c.step() {
        ResultCode::SQLITE_ROW => Ok(true),
        ResultCode::SQLITE_DONE => Ok(false),
        code => Err(io_error_from_sqlite(Some(db), code)),
    }
}

/// RAII-based transaction object. Without any further action, it will get rolled back.
struct Transaction<'a> {
    db: Option<&'a sqlite3::Database>,
}

impl<'a> Transaction<'a> {
    /// Starts the transaction.
    fn new(db: &'a sqlite3::Database) -> IoResult<Transaction<'a>> {
        match db.prepare("BEGIN;", &None).map(|mut c| step_cursor(db, &mut c)) {
            Ok(..) => Ok(Transaction { db: Some(db) }),
            Err(err) => Err(io_error_from_sqlite(Some(db), err)),
        }
    }

    /// Consumes the transaction while commiting it.
    fn commit(mut self) {
        let db = self.db.take().unwrap();
        match db.prepare("COMMIT;", &None).map(|mut c| step_cursor(db, &mut c)) {
            Ok(..) => {},
            Err(err) => fail_from_sqlite(db, err),
        }
    }
}

#[unsafe_destructor]
impl<'a> Drop for Transaction<'a> {
    fn drop(&mut self) {
        match self.db {
            Some(db) => match db.prepare("ROLLBACK;", &None).map(|mut c| step_cursor(db, &mut c)) {
                Ok(..) => {},
                Err(err) => fail_from_sqlite(db, err),
            },
            None => {}
        }
    }
}

/// The metadata cache backed by SQLite database.
///
/// The cache is used for either retrieving the directory contents (`get_entries`)
/// or retrieving the cached timeline based on the file's MD5 hash if any (`get_metadata`).
/// The former touches two tables `directories` (for the cached directory)
/// and `files` (for the cached directory contents);
/// the latter touches `files` (for the cached file hash if any) and `timelines` (for metadata).
/// This means that invalidating the directory contents will invalidate any related metadata if any.
pub struct MetadataCache {
    /// The predefined "root" path.
    ///
    /// This is used to normalize the in-database path, so that the cached timeline won't get
    /// invalidated when the files have been moved but the relative paths from the root path
    /// to the files haven't been changed.
    root: Path,
    /// The SQLite database.
    db: sqlite3::Database,
}

/// A value for `files.size` when the "file" is actually a directory.
const SIZE_FOR_DIRECTORY: i64 = -1;
/// A value for `files.size` when the "file" is actually not a file nor a directory.
const SIZE_FOR_NON_REGULAR: i64 = -2;

/// Calculates the value for `files.size` from given `stat` result.
fn size_from_filestat(st: &FileStat) -> i64 {
    match st.kind {
        FileType::RegularFile => st.size as i64,
        FileType::Directory => SIZE_FOR_DIRECTORY,
        _ => SIZE_FOR_NON_REGULAR,
    }
}

/// Makes an MD5 hash from given byte slice. The slice should be 16 bytes long.
fn md5_hash_from_slice(h: &[u8]) -> Option<MD5Hash> {
    if h.len() == 16 {
        Some(MD5Hash([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7],
                      h[8], h[9], h[10], h[11], h[12], h[13], h[14], h[15]]))
    } else {
        None
    }
}

/// The cache result from the database.
#[deriving(Show)]
enum CacheResult<T> {
    /// The cached entry is present and valid with given row ID and (partial) value.
    Valid(i64, T),
    /// The cached entry is present but invalid.
    Invalid(i64),
    /// There is no cached entry.
    None,
}

impl MetadataCache {
    /// Opens a metadata cache.
    pub fn open(root: Path, dbpath: &Path) -> IoResult<MetadataCache> {
        // avoid the initial `:`
        let mut path = dbpath.as_str().unwrap().into_cow();
        if path.as_slice().starts_with(":") {
            path = format!(".{}{}", path::SEP, path).into_cow();
        }

        let db = match sqlite3::open(path.as_slice()) {
            Ok(db) => db,
            Err(err) => { return Err(io_error_from_sqlite(None, err)); }
        };
        let mut db = MetadataCache { root: root, db: db };
        try!(db.create_schema());
        Ok(db)
    }

    /// Opens an in-memory metadata cache.
    pub fn open_in_memory(root: Path) -> IoResult<MetadataCache> {
        let db = match sqlite3::open(":memory:") {
            Ok(db) => db,
            Err(err) => { return Err(io_error_from_sqlite(None, err)); }
        };
        let mut db = MetadataCache { root: root, db: db };
        try!(db.create_schema());
        Ok(db)
    }

    /// `try!`-friendly version of `Database::prepare`.
    fn prepare<'a>(&'a self, sql: &str) -> IoResult<sqlite3::Cursor<'a>> {
        self.db.prepare(sql, &None).map_err(|err| io_error_from_sqlite(Some(&self.db), err))
    }

    /// `try!`-friendly version of `Database::exec`.
    fn exec(&mut self, sql: &str) -> IoResult<bool> {
        self.db.exec(sql).map_err(|err| io_error_from_sqlite(Some(&self.db), err))
    }

    /// Creates a required database schema.
    pub fn create_schema(&mut self) -> IoResult<()> {
        // TODO schema upgrade
        try!(self.exec("
            BEGIN;
            CREATE TABLE IF NOT EXISTS directories(
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                path BLOB NOT NULL UNIQUE, -- root-relative null-separated components
                mtime INTEGER NOT NULL -- msecs
            );
            CREATE TABLE IF NOT EXISTS files(
                id INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
                dir INTEGER NOT NULL REFERENCES directories(id),
                name BLOB NOT NULL,
                size INTEGER NOT NULL, -- negative for non-regular files or directories
                mtime INTEGER NOT NULL, -- msecs
                hash BLOB,
                UNIQUE (dir, name)
            );
            CREATE TABLE IF NOT EXISTS timelines(
                hash BLOB PRIMARY KEY NOT NULL,
                random_metadata INTEGER NOT NULL, -- nonzero when randomness affects metadata
                title TEXT,
                genre TEXT,
                artist TEXT,
                level INTEGER,
                levelsystem INTEGER,
                difficulty INTEGER
            );
            COMMIT;
        "));
        Ok(())
    }

    /// Checks the status of a cached list of entries in the directory.
    /// It may also return the current `stat` result, in order to avoid duplicate calls to `stat`.
    /// The caller is free to call this method during a transaction.
    fn check_cached_entries(&self, path: &Path, encoded: &[u8])
                            -> IoResult<(CacheResult<()>, Option<IoResult<FileStat>>)> {
        let mut c = try!(self.prepare("
            SELECT id, mtime FROM directories WHERE path = ?;
        "));
        c.bind_param(1, &BindArg::Blob(encoded.to_vec()));
        if try!(step_cursor(&self.db, &mut c)) {
            // check if the directory has the same mtime
            let dirid = c.get_i64(0);
            let dirmtime = c.get_i64(1);
            match path.stat() {
                Ok(st) if st.kind == FileType::Directory && st.modified as i64 == dirmtime =>
                    Ok((CacheResult::Valid(dirid, ()), Some(Ok(st)))),

                // DO NOT skip the error, this may indicate the required eviction
                st => Ok((CacheResult::Invalid(dirid), Some(st))),
            }
        } else {
            Ok((CacheResult::None, None))
        }
    }

    /// Retrieves a list of directories and non-directories in the directory if any.
    pub fn get_entries(&self, path: &Path) -> IoResult<(Vec<Path>, Vec<(Path, Option<MD5Hash>)>)> {
        debug!("get_entries: path = {}", path.display());

        let encoded = encode_path(&self.root, path);
        let mut dirs = Vec::new();
        let mut files = Vec::new();

        let tr = try!(Transaction::new(&self.db));

        let (res, dirstat) = try!(self.check_cached_entries(path, encoded[]));
        debug!("get_entries: cache result = {}", res);

        match res {
            CacheResult::Valid(dirid, ()) => {
                // retrieve entries from the cache
                let mut c = try!(self.prepare("
                    SELECT name, size, hash FROM files WHERE dir = ?;
                "));
                c.bind_param(1, &BindArg::Integer64(dirid));
                while try!(step_cursor(&self.db, &mut c)) {
                    let path = path.join(c.get_blob(0).unwrap_or("".as_bytes()));
                    let size = c.get_i64(1);
                    let hash = c.get_blob(2).and_then(md5_hash_from_slice);
                    if size >= 0 {
                        files.push((path, hash));
                    } else if size == SIZE_FOR_DIRECTORY {
                        dirs.push(path);
                    }
                }
                drop(c);
            }

            CacheResult::Invalid(..) | CacheResult::None => {
                let dirstat: IoResult<FileStat> = dirstat.unwrap_or_else(|| path.stat());
                debug!("get_entries: dirstat.modified = {}",
                       dirstat.as_ref().ok().map(|st| st.modified));

                // entries for the cached directory, if any, are now invalid.
                match res {
                    CacheResult::Invalid(dirid) => {
                        let mut c = try!(self.prepare("
                            DELETE FROM files WHERE dir = ?;
                        "));
                        c.bind_param(1, &BindArg::Integer64(dirid));
                        try!(step_cursor(&self.db, &mut c));
                        drop(c);
                    }
                    _ => {}
                }

                match (&dirstat, res) {
                    (&Ok(ref dirst), _) => {
                        // this *can* fail; the transaction would get rolled back then.
                        let entries: Vec<Path> = try!(readdir(path));
                        debug!("get_entries: entries = {}",
                               entries.iter().map(|p| p.display().to_string())
                                             .collect::<Vec<String>>());
                        let entrystats = try!(entries.iter().map(|p| p.stat())
                                                            .collect::<Result<Vec<FileStat>,_>>());
                        debug!("get_entries: entrystats.modified = {}",
                               entrystats.iter().map(|p| p.modified).collect::<Vec<u64>>());

                        // insert or replace the directory entry
                        let mut c = try!(self.prepare("
                            INSERT OR REPLACE INTO directories(path, mtime) VALUES(?, ?);
                        "));
                        c.bind_param(1, &BindArg::Blob(encoded));
                        c.bind_param(2, &BindArg::Integer64(dirst.modified as i64));
                        try!(step_cursor(&self.db, &mut c));
                        drop(c);
                        let dirid = self.db.get_last_insert_rowid();

                        // insert file entries
                        let mut c = try!(self.prepare("
                            INSERT INTO files(dir, name, size, mtime) VALUES(?, ?, ?, ?);
                        "));
                        c.bind_param(1, &BindArg::Integer64(dirid));
                        for (path, st) in entries.into_iter().zip(entrystats.iter()) {
                            let filename = path.filename().unwrap().to_vec();
                            c.reset();
                            c.bind_param(2, &BindArg::Blob(filename));
                            c.bind_param(3, &BindArg::Integer64(size_from_filestat(st)));
                            c.bind_param(4, &BindArg::Integer64(st.modified as i64));
                            try!(step_cursor(&self.db, &mut c));
                            match st.kind {
                                FileType::RegularFile => files.push((path, None)),
                                FileType::Directory => dirs.push(path),
                                _ => {}
                            }
                        }
                        drop(c);
                    }

                    (&Err(..), CacheResult::Invalid(dirid)) => {
                        // remove the directory entry if any
                        let mut c = try!(self.prepare("
                            DELETE FROM directories WHERE id = ?;
                        "));
                        c.bind_param(1, &BindArg::Integer64(dirid));
                        try!(step_cursor(&self.db, &mut c));
                        drop(c);
                    }

                    (_, _) => {}
                }

                tr.commit();

                // if stat failed we should return an IoError.
                try!(dirstat);
            }
        }

        debug!("get_entries: dirs = {}",
               dirs.iter().map(|p| p.display().to_string()).collect::<Vec<String>>());
        debug!("get_entries: files = {}",
               files.iter().map(|&(ref p,h)| (p.display(),h).to_string()).collect::<Vec<String>>());

        Ok((dirs, files))
    }

    /// Checks the status of a cached hash value of given file.
    /// `encoded_dir` should have been encoded with `encode_path`.
    /// It may also return the current `stat` result, in order to avoid duplicate calls to `stat`.
    /// The caller is free to call this method during a transaction.
    fn check_cached_hash(&self, path: &Path, encoded_dir: &[u8], name: &[u8])
                            -> IoResult<(CacheResult<MD5Hash>, Option<IoResult<FileStat>>)> {
        let mut c = try!(self.prepare("
            SELECT f.id, f.size, f.mtime, f.hash
            FROM directories d INNER JOIN files f ON d.id = f.dir
            WHERE d.path = ? AND f.name = ?;
        "));
        c.bind_param(1, &BindArg::Blob(encoded_dir.to_vec()));
        c.bind_param(2, &BindArg::Blob(name.to_vec()));
        if try!(step_cursor(&self.db, &mut c)) {
            // check if the file has the same mtime and size
            let fileid = c.get_i64(0);
            let filesize = c.get_i64(1);
            let filemtime = c.get_i64(2);
            let filehash = c.get_blob(3).and_then(md5_hash_from_slice);

            match filehash {
                None => Ok((CacheResult::Invalid(fileid), None)),
                Some(filehash) => match path.stat() {
                    Ok(st) if st.size as i64 == filesize && st.modified as i64 == filemtime =>
                        Ok((CacheResult::Valid(fileid, filehash), Some(Ok(st)))),
                    st => Ok((CacheResult::Invalid(fileid), Some(st))),
                }
            }
        } else {
            Ok((CacheResult::None, None))
        }
    }

    /// Retrieves a hash value of given file if any.
    /// It can also return a reference to the open file (rewound to the beginning) if possible.
    pub fn get_hash(&self, path: &Path) -> IoResult<(MD5Hash, Option<File>)> {
        debug!("get_hash: path = {}", path.display());

        let encoded = encode_path(&self.root, &path.dir_path());
        let filename = path.filename().unwrap_or("".as_bytes());

        let tr = try!(Transaction::new(&self.db));

        let (res, filestat) = try!(self.check_cached_hash(path, encoded[], filename));
        debug!("get_hash: cache result = {}", res);

        match res {
            CacheResult::Valid(_fileid, filehash) => Ok((filehash, None)),

            CacheResult::Invalid(..) | CacheResult::None => {
                let filestat: IoResult<FileStat> = filestat.unwrap_or_else(|| path.stat());
                debug!("get_hash: filestat.size = {}, filestat.modified = {}",
                       filestat.as_ref().ok().map(|st| st.size),
                       filestat.as_ref().ok().map(|st| st.modified));

                let mut f = try!(File::open(path));
                let hash = try!(MD5::from_reader(&mut f)).finish();

                match res {
                    CacheResult::Invalid(fileid) => {
                        let mut c = try!(self.prepare("
                            UPDATE files SET hash = ? WHERE id = ?;
                        "));
                        c.bind_param(1, &BindArg::Blob(hash.as_slice().to_vec()));
                        c.bind_param(2, &BindArg::Integer64(fileid));
                        try!(step_cursor(&self.db, &mut c));
                        drop(c);
                    }
                    _ => {}
                }

                tr.commit();
                try!(filestat); // if stat failed we should return an IoError.

                try!(f.seek(0, SeekSet));
                Ok((hash, Some(f)))
            }
        }
    }

    /// Retrieves a cached metadata for given hash if any.
    pub fn get_metadata(&self, hash: &MD5Hash) -> IoResult<Option<Meta>> {
        debug!("get_metadata: hash = {}", *hash);

        let mut c = try!(self.prepare("
            SELECT random_metadata, title, artist, genre, level, levelsystem, difficulty
            FROM timelines WHERE hash = ?;
        "));
        c.bind_param(1, &BindArg::Blob(hash.as_slice().to_vec()));
        if try!(step_cursor(&self.db, &mut c)) {
            let random = c.get_i64(0) != 0;
            let title = c.get_text(1).map(|s| s.to_string());
            let artist = c.get_text(2).map(|s| s.to_string());
            let genre = c.get_text(3).map(|s| s.to_string());
            let level = match (c.get_column_type(4), FromPrimitive::from_i64(c.get_i64(5))) {
                (ColumnType::SQLITE_INTEGER, Some(system)) =>
                    c.get_i64(4).to_int().map(|value| Level { value: value, system: system }),
                (_, _) => None
            };
            let difficulty = c.get_i64(6).to_int().map(Difficulty);
            Ok(Some(Meta {
                random: random,
                title: title, subtitles: Vec::new(), genre: genre, artist: artist,
                subartists: Vec::new(), comments: Vec::new(),
                level: level, difficulty: difficulty,
            }))
        } else {
            Ok(None)
        }
    }

    /// Stores a cached metadata for given hash.
    ///
    /// Unlike other methods, this does not go through a transition
    /// since parsing metadata is not a small task and it can harm the performance.
    pub fn put_metadata(&self, hash: &MD5Hash, meta: Meta) -> IoResult<()> {
        debug!("put_metadata: hash = {}", *hash);

        let (level, levelsystem) = match meta.level {
            Some(l) => (l.value.to_i64(), Some(l.system as i64)),
            None => (None, None)
        };

        let mut c = try!(self.prepare("
            INSERT OR REPLACE
            INTO timelines(hash, random_metadata, title, artist, genre,
                           level, levelsystem, difficulty)
            VALUES(?, ?, ?, ?, ?, ?, ?, ?);
        "));
        c.bind_param(1, &BindArg::Blob(hash.as_slice().to_vec()));
        c.bind_param(2, &BindArg::Integer64(if meta.random {1} else {0}));
        c.bind_param(3, &meta.title.map_or(BindArg::Null, BindArg::Text));
        c.bind_param(4, &meta.artist.map_or(BindArg::Null, BindArg::Text));
        c.bind_param(5, &meta.genre.map_or(BindArg::Null, BindArg::Text));
        c.bind_param(6, &level.map_or(BindArg::Null, BindArg::Integer64));
        c.bind_param(7, &levelsystem.map_or(BindArg::Null, BindArg::Integer64));
        c.bind_param(8, &meta.difficulty.map_or(BindArg::Null,
                                                |Difficulty(v)| BindArg::Integer64(v as i64)));
        try!(step_cursor(&self.db, &mut c));
        drop(c);

        Ok(())
    }
}

