// SPDX-License-Identifier: MIT

use std::path::PathBuf;

use gtk::{gio, prelude::*};


const INVALID_UNLOCK_VOLUME_OPERAND: &str = "invalid --unlock-volume operand";


#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnlockTarget {
    pub unix_device: Option<PathBuf>,
    pub uuid: Option<String>,
}

impl UnlockTarget {
    pub fn parse(operand: &str) -> Result<Self, &'static str> {
        if operand.is_empty() {
            return Err(INVALID_UNLOCK_VOLUME_OPERAND);
        }
        if operand.starts_with('/') {
            return Ok(Self {
                unix_device: Some(PathBuf::from(operand)),
                uuid: None,
            });
        }
        match super::devices::normalize_luks_uuid(operand) {
            Some(uuid) => Ok(Self {
                unix_device: None,
                uuid: Some(uuid),
            }),
            None => Err(INVALID_UNLOCK_VOLUME_OPERAND),
        }
    }
}



// Subscribe before scanning so a newly appearing volume cannot be missed.




pub(crate) enum ResolvedUnlock {
    Volume(gio::Volume),
    PasswordDrive(gio::Drive),
    NotEncrypted,
    Unmatched,
}

pub(crate) fn resolve_unlock_target(target: &UnlockTarget, monitor: &gio::VolumeMonitor) -> ResolvedUnlock {
    let volumes = monitor.volumes();
    let volume_identities: Vec<_> = volumes.iter().map(volume_unlock_identity).collect();
    let drives = super::orphaned_password_drives(&volumes, &monitor.connected_drives());
    let drive_identities: Vec<_> = drives.iter().map(password_drive_identity).collect();
    match classify_unlock_identities(target, &volume_identities, &drive_identities) {
        UnlockResolution::EncryptedVolume(index) => ResolvedUnlock::Volume(volumes[index].clone()),
        UnlockResolution::PasswordDrive(index) => {
            ResolvedUnlock::PasswordDrive(drives[index].clone())
        }
        UnlockResolution::NotEncrypted => ResolvedUnlock::NotEncrypted,
        UnlockResolution::Unmatched => ResolvedUnlock::Unmatched,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VolumeUnlockIdentity {
    unix_device: Option<String>,
    uuid: Option<String>,
    crypto_uuid: Option<String>,
    encrypted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PasswordDriveIdentity {
    unix_device: Option<String>,
    uuid: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UnlockResolution {
    EncryptedVolume(usize),
    PasswordDrive(usize),
    NotEncrypted,
    Unmatched,
}

fn classify_unlock_identities(
    target: &UnlockTarget,
    volumes: &[VolumeUnlockIdentity],
    password_drives: &[PasswordDriveIdentity],
) -> UnlockResolution {
    if let Some(device) = target.unix_device.as_ref()
        && let Some((index, volume)) = volumes.iter().enumerate().find(|(_, volume)| {
            volume
                .unix_device
                .as_ref()
                .is_some_and(|unix| std::path::Path::new(unix) == device)
        })
    {
        return if volume.encrypted {
            UnlockResolution::EncryptedVolume(index)
        } else {
            UnlockResolution::NotEncrypted
        };
    }

    if let Some(target_uuid) = target.uuid.as_deref()
        && let Some((index, volume)) = volumes.iter().enumerate().find(|(_, volume)| {
            uuid_matches(target_uuid, volume.uuid.as_deref())
                || uuid_matches(target_uuid, volume.crypto_uuid.as_deref())
        })
    {
        return if volume.encrypted {
            UnlockResolution::EncryptedVolume(index)
        } else {
            UnlockResolution::NotEncrypted
        };
    }

    if let Some((index, _)) = password_drives
        .iter()
        .enumerate()
        .find(|(_, drive)| password_drive_matches(target, drive))
    {
        return UnlockResolution::PasswordDrive(index);
    }

    UnlockResolution::Unmatched
}

fn uuid_matches(target_uuid: &str, candidate: Option<&str>) -> bool {
    let Some(candidate) = candidate else {
        return false;
    };
    let Some(normalized_target) = super::devices::normalize_luks_uuid(target_uuid) else {
        return false;
    };
    super::devices::normalize_luks_uuid(candidate).as_deref() == Some(normalized_target.as_str())
}

fn password_drive_matches(target: &UnlockTarget, drive: &PasswordDriveIdentity) -> bool {
    if let Some(device) = target.unix_device.as_ref() {
        return drive
            .unix_device
            .as_ref()
            .is_some_and(|unix| std::path::Path::new(unix) == device);
    }
    if let Some(uuid) = target.uuid.as_deref() {
        return uuid_matches(uuid, drive.uuid.as_deref());
    }
    false
}

fn volume_unlock_identity(volume: &gio::Volume) -> VolumeUnlockIdentity {
    VolumeUnlockIdentity {
        unix_device: super::gio_volume_unix_device(volume).map(|device| device.to_string()),
        uuid: volume.uuid().map(|uuid| uuid.to_string()),
        crypto_uuid: super::crypto_password_uuid_for_volume(volume),
        encrypted: super::gio_volume_is_encrypted(volume),
    }
}

fn password_drive_identity(drive: &gio::Drive) -> PasswordDriveIdentity {
    PasswordDriveIdentity {
        unix_device: drive
            .identifier(gio::VOLUME_IDENTIFIER_KIND_UNIX_DEVICE.as_str())
            .map(|device| device.to_string()),
        uuid: drive
            .identifier(gio::VOLUME_IDENTIFIER_KIND_UUID.as_str())
            .map(|uuid| uuid.to_string()),
    }
}








#[cfg(test)]
mod tests;
