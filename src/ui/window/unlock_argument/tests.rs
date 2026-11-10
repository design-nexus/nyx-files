// SPDX-License-Identifier: MIT


use super::*;

const HYPHENATED_UUID: &str = "6e5d75a7-e4e2-4c7d-9c1c-8e5a5e5d75a7";
const COMPACT_UUID: &str = "6e5d75a7e4e24c7d9c1c8e5a5e5d75a7";




fn encrypted_volume(
    unix_device: &str,
    uuid: Option<&str>,
    crypto_uuid: Option<&str>,
) -> VolumeUnlockIdentity {
    VolumeUnlockIdentity {
        unix_device: Some(unix_device.to_owned()),
        uuid: uuid.map(str::to_owned),
        crypto_uuid: crypto_uuid.map(str::to_owned),
        encrypted: true,
    }
}

fn unencrypted_volume(unix_device: &str, uuid: Option<&str>) -> VolumeUnlockIdentity {
    VolumeUnlockIdentity {
        unix_device: Some(unix_device.to_owned()),
        uuid: uuid.map(str::to_owned),
        crypto_uuid: None,
        encrypted: false,
    }
}

#[test]
fn identities_unix_device_uuid_and_not_encrypted() {
    let target = |operand: &str| UnlockTarget::parse(operand).expect("unlock operand");
    let password_drive = [PasswordDriveIdentity {
        unix_device: Some("/dev/sdb".to_owned()),
        uuid: Some(COMPACT_UUID.to_owned()),
    }];
    assert_eq!(
        classify_unlock_identities(
            &target("/dev/sdb1"),
            &[
                unencrypted_volume("/dev/sda1", None),
                encrypted_volume("/dev/sdb1", Some(HYPHENATED_UUID), None),
            ],
            &[],
        ),
        UnlockResolution::EncryptedVolume(1),
        "unix-device should match the encrypted volume"
    );
    assert_eq!(
        classify_unlock_identities(
            &target(COMPACT_UUID),
            &[encrypted_volume("/dev/sdb1", Some(HYPHENATED_UUID), None)],
            &[],
        ),
        UnlockResolution::EncryptedVolume(0),
        "compact UUID should match a hyphenated volume UUID"
    );
    assert_eq!(
        classify_unlock_identities(
            &target(COMPACT_UUID),
            &[encrypted_volume("/dev/sdb1", None, Some(HYPHENATED_UUID))],
            &[],
        ),
        UnlockResolution::EncryptedVolume(0),
        "compact UUID should match crypto_uuid"
    );
    assert_eq!(
        classify_unlock_identities(&target("/dev/sdb"), &[], &password_drive),
        UnlockResolution::PasswordDrive(0),
        "unix-device should match a password drive"
    );
    assert_eq!(
        classify_unlock_identities(&target(HYPHENATED_UUID), &[], &password_drive),
        UnlockResolution::PasswordDrive(0),
        "UUID should match a password drive"
    );
    assert_eq!(
        classify_unlock_identities(
            &target("/dev/sdb1"),
            &[unencrypted_volume("/dev/sdb1", Some(HYPHENATED_UUID))],
            &[],
        ),
        UnlockResolution::NotEncrypted,
        "matching unencrypted volume should not unlock"
    );
    assert_eq!(
        classify_unlock_identities(
            &target("/dev/sdb1"),
            &[
                encrypted_volume("/dev/sdc1", Some(HYPHENATED_UUID), None),
                unencrypted_volume("/dev/sdb1", None),
            ],
            &[],
        ),
        UnlockResolution::NotEncrypted,
        "unix-device match should win over a different volume's UUID"
    );
}



