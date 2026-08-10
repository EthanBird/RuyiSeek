use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const SYSTEM_PREFIXES: &[&str] = &[
    "/boot", "/dev", "/opt", "/proc", "/snap", "/sys", "/tmp", "/usr", "/var",
];

/// Return the user's home plus readable, local data volumes visible in this mount namespace.
///
/// Linux exposes the authoritative list in `/proc/self/mountinfo`. System, pseudo, network,
/// loop-backed and home-ancestor mounts are deliberately omitted so automatic discovery never
/// expands into `/`, `/home`, `/boot` or another operating-system tree.
///
/// # Errors
///
/// Returns the `/proc/self/mountinfo` read error when the kernel mount table is unavailable.
pub fn discover_default_roots(home: &Path) -> io::Result<Vec<PathBuf>> {
    let mountinfo = fs::read(MOUNTINFO_PATH)?;
    let mut roots = roots_from_mountinfo(home, &mountinfo);
    roots.retain(|root| root == home || is_readable_directory(root));
    Ok(roots)
}

fn is_readable_directory(path: &Path) -> bool {
    path.is_dir() && fs::read_dir(path).is_ok()
}

fn roots_from_mountinfo(home: &Path, mountinfo: &[u8]) -> Vec<PathBuf> {
    let mut roots = vec![home.to_path_buf()];
    let mut seen_mounts = HashSet::new();

    for line in mountinfo.split(|byte| *byte == b'\n') {
        let Some(separator) = find_bytes(line, b" - ") else {
            continue;
        };
        let left: Vec<_> = line[..separator]
            .split(u8::is_ascii_whitespace)
            .filter(|field| !field.is_empty())
            .collect();
        let right: Vec<_> = line[separator + 3..]
            .split(u8::is_ascii_whitespace)
            .filter(|field| !field.is_empty())
            .collect();
        if left.len() < 5 || right.len() < 2 {
            continue;
        }

        let filesystem = right[0];
        let source = decode_mount_field(right[1]);
        let mount_root = decode_mount_field(left[3]);
        let mount_point = PathBuf::from(decode_mount_field(left[4]));
        if !is_local_data_mount(home, &mount_point, filesystem, &source) {
            continue;
        }

        // A bind mount is identified by the same major:minor device and the same root within
        // that filesystem. Index it only once even if it is exposed at multiple paths.
        let identity = (left[2].to_vec(), mount_root.as_os_str().as_bytes().to_vec());
        if seen_mounts.insert(identity) {
            roots.push(mount_point);
        }
    }

    roots[1..].sort();
    roots.dedup();
    roots
}

fn is_local_data_mount(home: &Path, mount_point: &Path, filesystem: &[u8], source: &OsStr) -> bool {
    let source = source.as_bytes();
    if !source.starts_with(b"/dev/") || source.starts_with(b"/dev/loop") {
        return false;
    }
    if is_excluded_filesystem(filesystem) || is_system_mount(mount_point) {
        return false;
    }

    // HOME itself is already a root. Reject ancestors such as a separate /home filesystem so
    // automatic discovery never exposes other users. Descendant mounts stay in the set: their
    // appearance must trigger a rebuild, and the scanner treats configured roots as boundaries.
    mount_point != home && !home.starts_with(mount_point)
}

fn is_excluded_filesystem(filesystem: &[u8]) -> bool {
    matches!(
        filesystem,
        b"proc"
            | b"sysfs"
            | b"devtmpfs"
            | b"devpts"
            | b"tmpfs"
            | b"cgroup"
            | b"cgroup2"
            | b"overlay"
            | b"squashfs"
            | b"nfs"
            | b"nfs4"
            | b"cifs"
            | b"smb3"
            | b"fuse.sshfs"
    )
}

fn is_system_mount(path: &Path) -> bool {
    if path == Path::new("/") {
        return true;
    }

    if SYSTEM_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        return true;
    }

    path.starts_with("/run") && !path.starts_with("/run/media")
}

fn decode_mount_field(field: &[u8]) -> OsString {
    let mut decoded = Vec::with_capacity(field.len());
    let mut index = 0;
    while index < field.len() {
        if field[index] == b'\\' && index + 3 < field.len() {
            let octal = &field[index + 1..index + 4];
            if octal.iter().all(|byte| matches!(byte, b'0'..=b'7')) {
                let value = u16::from(octal[0] - b'0') * 64
                    + u16::from(octal[1] - b'0') * 8
                    + u16::from(octal[2] - b'0');
                if let Ok(value) = u8::try_from(value) {
                    decoded.push(value);
                    index += 4;
                    continue;
                }
            }
        }
        decoded.push(field[index]);
        index += 1;
    }
    OsString::from_vec(decoded)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UOS_MOUNTINFO: &[u8] = br"29 23 8:2 / / rw,relatime - ext4 /dev/sda2 rw
31 29 0:5 / /proc rw,nosuid,nodev,noexec,relatime - proc proc rw
32 29 8:1 / /boot rw,relatime - ext4 /dev/sda1 rw
33 29 8:3 / /home rw,relatime - ext4 /dev/sda3 rw
40 29 8:17 / /media/alice/Work rw,nosuid,nodev,relatime - ext4 /dev/sdb1 rw
41 29 8:33 / /run/media/alice/My\040Disk rw,nosuid,nodev,relatime - exfat /dev/sdc1 rw
42 29 253:0 / /data rw,relatime - xfs /dev/mapper/data rw
43 29 7:0 / /snap/core/1 ro,nodev,relatime - squashfs /dev/loop0 ro
44 29 0:42 / /mnt/company rw,relatime - nfs4 server:/company rw
45 29 8:17 / /mnt/work-bind rw,relatime - ext4 /dev/sdb1 rw
46 29 8:49 / /home/alice/external rw,relatime - ext4 /dev/sdd1 rw
";

    #[test]
    fn discovers_local_data_volumes_and_decodes_mount_paths() {
        let roots = roots_from_mountinfo(Path::new("/home/alice"), UOS_MOUNTINFO);

        assert_eq!(
            roots,
            vec![
                PathBuf::from("/home/alice"),
                PathBuf::from("/data"),
                PathBuf::from("/home/alice/external"),
                PathBuf::from("/media/alice/Work"),
                PathBuf::from("/run/media/alice/My Disk"),
            ]
        );
    }

    #[test]
    fn rejects_os_network_loop_and_home_related_mounts() {
        let roots = roots_from_mountinfo(Path::new("/home/alice"), UOS_MOUNTINFO);

        for excluded in [
            "/",
            "/boot",
            "/home",
            "/snap/core/1",
            "/mnt/company",
            "/mnt/work-bind",
        ] {
            assert!(
                !roots.contains(&PathBuf::from(excluded)),
                "included {excluded}"
            );
        }
    }

    #[test]
    fn malformed_lines_are_ignored() {
        let roots = roots_from_mountinfo(
            Path::new("/home/alice"),
            b"not mountinfo\n50 29 too-short - ext4 /dev/sdz1 rw\n",
        );
        assert_eq!(roots, vec![PathBuf::from("/home/alice")]);
    }
}
