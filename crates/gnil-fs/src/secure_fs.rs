use std::{
    ffi::{OsStr, OsString},
    io,
    os::{fd::OwnedFd, unix::ffi::OsStringExt as _},
    path::{Component, Path, PathBuf},
};

use nix::{
    dir::Dir,
    errno::Errno,
    fcntl::{AtFlags, OFlag, RenameFlags, open, openat, readlinkat, renameat2},
    sys::stat::{Mode, SFlag, fchmod, fstat, fstatat, mkdirat},
    unistd::{UnlinkatFlags, symlinkat, unlinkat},
};

const DIRECTORY_FLAGS: OFlag = OFlag::O_RDONLY
    .union(OFlag::O_DIRECTORY)
    .union(OFlag::O_CLOEXEC)
    .union(OFlag::O_NOFOLLOW);

pub(crate) struct Parent {
    pub(crate) dir: OwnedFd,
    pub(crate) name: OsString,
}

pub(crate) fn open_dir(path: &Path) -> io::Result<OwnedFd> {
    let mut current = if path.is_absolute() {
        open(Path::new("/"), DIRECTORY_FLAGS, Mode::empty())
    } else {
        open(Path::new("."), DIRECTORY_FLAGS, Mode::empty())
    }
    .map_err(io::Error::from)?;

    for component in path.components() {
        let name = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => name,
            Component::ParentDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unsafe path component in {}", path.display()),
                ));
            }
        };
        current =
            openat(&current, name, DIRECTORY_FLAGS, Mode::empty()).map_err(io::Error::from)?;
    }
    Ok(current)
}

pub(crate) fn ensure_dir(path: &Path, mode: u32) -> io::Result<OwnedFd> {
    let mut current = if path.is_absolute() {
        open(Path::new("/"), DIRECTORY_FLAGS, Mode::empty())
    } else {
        open(Path::new("."), DIRECTORY_FLAGS, Mode::empty())
    }
    .map_err(io::Error::from)?;

    for component in path.components() {
        let name = match component {
            Component::RootDir | Component::CurDir => continue,
            Component::Normal(name) => name,
            Component::ParentDir | Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unsafe path component in {}", path.display()),
                ));
            }
        };
        current = match openat(&current, name, DIRECTORY_FLAGS, Mode::empty()) {
            Ok(directory) => directory,
            Err(Errno::ENOENT) => {
                match mkdirat(&current, name, Mode::from_bits_truncate(mode)) {
                    Ok(()) | Err(Errno::EEXIST) => {}
                    Err(error) => return Err(io::Error::from(error)),
                }
                openat(&current, name, DIRECTORY_FLAGS, Mode::empty()).map_err(io::Error::from)?
            }
            Err(error) => return Err(io::Error::from(error)),
        };
    }
    Ok(current)
}

pub(crate) fn open_parent(path: &Path) -> io::Result<Parent> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?
        .to_os_string();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    Ok(Parent {
        dir: open_dir(parent)?,
        name,
    })
}

pub(crate) fn list_dir(path: &Path) -> io::Result<Vec<OsString>> {
    let mut directory = Dir::from_fd(open_dir(path)?).map_err(io::Error::from)?;
    directory
        .iter()
        .map(|entry| entry.map_err(io::Error::from))
        .filter_map(|entry| match entry {
            Ok(entry)
                if entry.file_name().to_bytes() != b"."
                    && entry.file_name().to_bytes() != b".." =>
            {
                Some(Ok(OsString::from_vec(
                    entry.file_name().to_bytes().to_vec(),
                )))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

pub(crate) fn exists(parent: &Parent) -> io::Result<bool> {
    match fstatat(
        &parent.dir,
        parent.name.as_os_str(),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(_) => Ok(true),
        Err(Errno::ENOENT) => Ok(false),
        Err(error) => Err(io::Error::from(error)),
    }
}

pub(crate) fn rename_noreplace(from: &Parent, to: &Parent) -> io::Result<()> {
    renameat2(
        &from.dir,
        from.name.as_os_str(),
        &to.dir,
        to.name.as_os_str(),
        RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(io::Error::from)
}

pub(crate) fn rename_exchange(first: &Parent, second: &Parent) -> io::Result<()> {
    renameat2(
        &first.dir,
        first.name.as_os_str(),
        &second.dir,
        second.name.as_os_str(),
        RenameFlags::RENAME_EXCHANGE,
    )
    .map_err(io::Error::from)
}

pub(crate) fn open_file_readonly(parent: &Parent) -> io::Result<OwnedFd> {
    openat(
        &parent.dir,
        parent.name.as_os_str(),
        OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
        Mode::empty(),
    )
    .map_err(io::Error::from)
}

pub(crate) fn create_file(parent: &Parent, mode: u32) -> io::Result<OwnedFd> {
    openat(
        &parent.dir,
        parent.name.as_os_str(),
        OFlag::O_WRONLY | OFlag::O_CLOEXEC | OFlag::O_CREAT | OFlag::O_EXCL | OFlag::O_NOFOLLOW,
        Mode::from_bits_truncate(mode),
    )
    .map_err(io::Error::from)
}

pub(crate) fn chmod_nofollow(
    path: &Path,
    mode: u32,
    expected_identity: (u64, u64),
) -> io::Result<()> {
    let parent = open_parent(path)?;
    let fd = openat(
        &parent.dir,
        parent.name.as_os_str(),
        OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|error| {
        if error == Errno::ELOOP {
            io::Error::new(io::ErrorKind::InvalidInput, "refusing to chmod symlink")
        } else {
            io::Error::from(error)
        }
    })?;
    let stat = fstat(&fd).map_err(io::Error::from)?;
    if (stat.st_dev, stat.st_ino) != expected_identity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "path changed while changing permissions: {}",
                path.display()
            ),
        ));
    }
    fchmod(&fd, Mode::from_bits_truncate(mode)).map_err(io::Error::from)
}

pub(crate) fn create_dir(parent: &Parent, mode: u32) -> io::Result<OwnedFd> {
    mkdirat(
        &parent.dir,
        parent.name.as_os_str(),
        Mode::from_bits_truncate(mode),
    )
    .map_err(io::Error::from)?;
    openat(
        &parent.dir,
        parent.name.as_os_str(),
        DIRECTORY_FLAGS,
        Mode::empty(),
    )
    .map_err(io::Error::from)
}

pub(crate) fn read_link(parent: &Parent) -> io::Result<OsString> {
    readlinkat(&parent.dir, parent.name.as_os_str()).map_err(io::Error::from)
}

pub(crate) fn create_symlink(target: &OsStr, parent: &Parent) -> io::Result<()> {
    symlinkat(target, &parent.dir, parent.name.as_os_str()).map_err(io::Error::from)
}

pub(crate) fn remove_tree(parent: &Parent) -> io::Result<()> {
    let logical_path = PathBuf::from(&parent.name);
    remove_tree_at(parent, &logical_path, &mut || Ok(()), &mut |_| {})
}

pub(crate) fn remove_tree_controlled(
    path: &Path,
    checkpoint: &mut dyn FnMut() -> io::Result<()>,
    removed: &mut dyn FnMut(&Path),
) -> io::Result<()> {
    let parent = open_parent(path)?;
    remove_tree_at(&parent, path, checkpoint, removed)
}

fn remove_tree_at(
    parent: &Parent,
    logical_path: &Path,
    checkpoint: &mut dyn FnMut() -> io::Result<()>,
    removed: &mut dyn FnMut(&Path),
) -> io::Result<()> {
    checkpoint()?;
    let stat = fstatat(
        &parent.dir,
        parent.name.as_os_str(),
        AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(io::Error::from)?;
    let kind = SFlag::from_bits_truncate(stat.st_mode);
    if !kind.contains(SFlag::S_IFDIR) {
        unlinkat(
            &parent.dir,
            parent.name.as_os_str(),
            UnlinkatFlags::NoRemoveDir,
        )
        .map_err(io::Error::from)?;
        removed(logical_path);
        return Ok(());
    }

    let directory_fd = openat(
        &parent.dir,
        parent.name.as_os_str(),
        DIRECTORY_FLAGS,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    let mut directory = Dir::from_fd(directory_fd).map_err(io::Error::from)?;
    let names = directory
        .iter()
        .map(|entry| entry.map_err(io::Error::from))
        .filter_map(|entry| match entry {
            Ok(entry)
                if entry.file_name().to_bytes() != b"."
                    && entry.file_name().to_bytes() != b".." =>
            {
                Some(Ok(OsString::from_vec(
                    entry.file_name().to_bytes().to_vec(),
                )))
            }
            Ok(_) => None,
            Err(error) => Some(Err(error)),
        })
        .collect::<io::Result<Vec<_>>>()?;

    for name in names {
        checkpoint()?;
        let child_path = logical_path.join(&name);
        let child = Parent {
            dir: openat(&directory, Path::new("."), DIRECTORY_FLAGS, Mode::empty())
                .map_err(io::Error::from)?,
            name,
        };
        remove_tree_at(&child, &child_path, checkpoint, removed)?;
    }
    unlinkat(
        &parent.dir,
        parent.name.as_os_str(),
        UnlinkatFlags::RemoveDir,
    )
    .map_err(io::Error::from)?;
    removed(logical_path);
    Ok(())
}
