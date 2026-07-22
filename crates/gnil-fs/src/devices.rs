use std::{collections::HashMap, path::PathBuf, thread};

use crossbeam_channel::{Receiver, TryRecvError, unbounded};
use nix::sys::statvfs::statvfs;
use thiserror::Error;
use zbus::{
    MatchRule,
    blocking::{Connection, MessageIterator, Proxy},
    fdo::ManagedObjects,
    message::Type,
    zvariant::{OwnedObjectPath, Value},
};

const SERVICE: &str = "org.freedesktop.UDisks2";
const ROOT: &str = "/org/freedesktop/UDisks2";
const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";
const BLOCK: &str = "org.freedesktop.UDisks2.Block";
const FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
const DRIVE: &str = "org.freedesktop.UDisks2.Drive";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceKind {
    Usb,
    SolidState,
    HardDisk,
    Other,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceEntry {
    pub id: String,
    pub drive_id: String,
    pub label: String,
    pub device_path: PathBuf,
    pub mount_path: Option<PathBuf>,
    pub kind: DeviceKind,
    pub size: u64,
    pub available: Option<u64>,
    pub removable: bool,
    pub can_eject: bool,
}

#[derive(Debug, Error)]
pub enum DeviceError {
    #[error("UDisks2 is unavailable: {0}")]
    Bus(#[from] zbus::Error),
    #[error("device monitor could not start: {0}")]
    Monitor(String),
}

pub struct DeviceMonitor {
    receiver: Receiver<()>,
}

impl DeviceMonitor {
    pub fn start() -> Result<Self, DeviceError> {
        let connection = Connection::system()?;
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(SERVICE)
            .map_err(|error| DeviceError::Monitor(error.to_string()))?
            .path_namespace(ROOT)
            .map_err(|error| DeviceError::Monitor(error.to_string()))?
            .build();
        let mut messages = MessageIterator::for_match_rule(rule, &connection, Some(32))?;
        let (sender, receiver) = unbounded();
        thread::Builder::new()
            .name("gnil-udisks-monitor".into())
            .spawn(move || {
                for message in &mut messages {
                    if message.is_err() || sender.send(()).is_err() {
                        break;
                    }
                }
            })
            .map_err(|error| DeviceError::Monitor(error.to_string()))?;
        Ok(Self { receiver })
    }

    #[must_use]
    pub fn take_changed(&self) -> bool {
        let mut changed = false;
        loop {
            match self.receiver.try_recv() {
                Ok(()) => changed = true,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return changed,
            }
        }
    }
}

pub fn scan_devices() -> Result<Vec<DeviceEntry>, DeviceError> {
    let connection = Connection::system()?;
    let manager = Proxy::new(&connection, SERVICE, ROOT, OBJECT_MANAGER)?;
    let objects: ManagedObjects = manager.call("GetManagedObjects", &())?;
    let mut devices = Vec::new();
    for (object_path, interfaces) in objects {
        if !interfaces.keys().any(|name| name.as_str() == FILESYSTEM)
            || !interfaces.keys().any(|name| name.as_str() == BLOCK)
        {
            continue;
        }
        let path = object_path.as_str();
        let block = Proxy::new(&connection, SERVICE, path, BLOCK)?;
        let id_usage: String = block.get_property("IdUsage")?;
        let hint_ignore: bool = block.get_property("HintIgnore")?;
        if id_usage != "filesystem" || hint_ignore {
            continue;
        }
        let hint_system: bool = block.get_property("HintSystem")?;
        let label: String = block.get_property("IdLabel")?;
        let device: Vec<u8> = block.get_property("Device")?;
        let block_size: u64 = block.get_property("Size")?;
        let drive_path: OwnedObjectPath = block.get_property("Drive")?;
        if drive_path.as_str() == "/" {
            continue;
        }
        let filesystem = Proxy::new(&connection, SERVICE, path, FILESYSTEM)?;
        let mount_points: Vec<Vec<u8>> = filesystem.get_property("MountPoints")?;
        let mount_path = mount_points
            .first()
            .and_then(|path| nul_terminated_path(path));

        let drive = Proxy::new(&connection, SERVICE, drive_path.as_str(), DRIVE)?;
        let removable: bool = drive.get_property("Removable")?;
        let media_removable: bool = drive.get_property("MediaRemovable")?;
        let can_eject: bool = drive.get_property("Ejectable")?;
        let connection_bus: String = drive.get_property("ConnectionBus")?;
        let rotation_rate: u32 = drive.get_property("RotationRate")?;
        if hint_system && !removable && !media_removable && connection_bus != "usb" {
            continue;
        }
        let device_path = nul_terminated_path(&device).unwrap_or_default();
        let display_label = if label.is_empty() {
            device_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            label
        };
        let filesystem_size: u64 = filesystem.get_property("Size").unwrap_or(0);
        let size = filesystem_size.max(block_size);
        let available = mount_path.as_ref().and_then(|mount| {
            statvfs(mount.as_path()).ok().map(|stats| {
                stats
                    .blocks_available()
                    .saturating_mul(stats.fragment_size())
            })
        });
        devices.push(DeviceEntry {
            id: path.to_owned(),
            drive_id: drive_path.as_str().to_owned(),
            label: display_label,
            device_path,
            mount_path,
            kind: classify_device(&connection_bus, rotation_rate, removable || media_removable),
            size,
            available,
            removable: removable || media_removable,
            can_eject,
        });
    }
    devices.sort_by_key(|device| device.label.to_lowercase());
    Ok(devices)
}

pub fn mount_device(object_path: &str) -> Result<PathBuf, DeviceError> {
    let connection = Connection::system()?;
    let filesystem = Proxy::new(&connection, SERVICE, object_path, FILESYSTEM)?;
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    let path: String = filesystem.call("Mount", &(options,))?;
    Ok(PathBuf::from(path))
}

pub fn unmount_device(object_path: &str) -> Result<(), DeviceError> {
    let connection = Connection::system()?;
    let filesystem = Proxy::new(&connection, SERVICE, object_path, FILESYSTEM)?;
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    let _: () = filesystem.call("Unmount", &(options,))?;
    Ok(())
}

pub fn eject_device(drive_path: &str) -> Result<(), DeviceError> {
    let connection = Connection::system()?;
    let drive = Proxy::new(&connection, SERVICE, drive_path, DRIVE)?;
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    let _: () = drive.call("Eject", &(options,))?;
    Ok(())
}

fn nul_terminated_path(bytes: &[u8]) -> Option<PathBuf> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    (!bytes[..end].is_empty()).then(|| {
        use std::os::unix::ffi::OsStringExt as _;
        PathBuf::from(std::ffi::OsString::from_vec(bytes[..end].to_vec()))
    })
}

fn classify_device(connection_bus: &str, rotation_rate: u32, removable: bool) -> DeviceKind {
    if connection_bus == "usb" || removable {
        DeviceKind::Usb
    } else if rotation_rate == 0 {
        DeviceKind::SolidState
    } else if rotation_rate > 0 {
        DeviceKind::HardDisk
    } else {
        DeviceKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_devices_without_needing_dbus() {
        assert_eq!(classify_device("usb", 0, false), DeviceKind::Usb);
        assert_eq!(classify_device("", 0, false), DeviceKind::SolidState);
        assert_eq!(classify_device("ata", 7200, false), DeviceKind::HardDisk);
    }

    #[test]
    fn decodes_nul_terminated_udisks_paths() {
        assert_eq!(
            nul_terminated_path(b"/run/media/user/disk\0ignored"),
            Some(PathBuf::from("/run/media/user/disk"))
        );
        assert_eq!(nul_terminated_path(b"\0"), None);
    }
}
